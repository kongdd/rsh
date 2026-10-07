use std::{collections::HashMap, sync::Arc};

use anyhow::{bail, Context, Result};
use tokio::{
    io::{split, AsyncRead, AsyncWrite, BufReader},
    net::TcpListener,
    sync::{mpsc, oneshot, Mutex},
    time::{interval, timeout, Duration},
};

use crate::{
    proto::{recv, send, Msg, Role, MAX_CMD},
    tls,
};

const AUTH_TIMEOUT: Duration = Duration::from_secs(5);

type Peers = Arc<Mutex<HashMap<String, mpsc::Sender<Job>>>>;

struct Job {
    cmd: String,
    tx: oneshot::Sender<Msg>,
}

struct Tokens {
    daemon: String,
    control: String,
}

pub async fn run() -> Result<()> {
    let bind = std::env::var("RSH_BIND").unwrap_or_else(|_| "0.0.0.0:7280".into());
    let tokens = Arc::new(Tokens {
        daemon: std::env::var("RSH_DAEMON_TOKEN").context("RSH_DAEMON_TOKEN is required")?,
        control: std::env::var("RSH_CONTROL_TOKEN").context("RSH_CONTROL_TOKEN is required")?,
    });
    let acceptor = tls::acceptor()?;
    let peers: Peers = Default::default();
    let listener = TcpListener::bind(&bind).await?;
    eprintln!("listening with TLS on {bind}");

    loop {
        let (tcp, _) = listener.accept().await?;
        let (peers, tokens, acceptor) = (peers.clone(), tokens.clone(), acceptor.clone());
        tokio::spawn(async move {
            let result = async {
                let stream = timeout(AUTH_TIMEOUT, acceptor.accept(tcp)).await??;
                handle(stream, peers, tokens).await
            }.await;
            if let Err(e) = result { eprintln!("{e}") }
        });
    }
}

async fn handle<S>(stream: S, peers: Peers, tokens: Arc<Tokens>) -> Result<()>
where
    S: AsyncRead + AsyncWrite + Unpin,
{
    let (r, mut w) = split(stream);
    let mut r = BufReader::new(r);

    let (token, role) = match timeout(AUTH_TIMEOUT, recv(&mut r)).await?? {
        Some(Msg::Auth { token, role }) => (token, role),
        _ => bail!("auth failed"),
    };
    let expected = match role {
        Role::Daemon => &tokens.daemon,
        Role::Control => &tokens.control,
    };
    if token != *expected {
        bail!("auth failed");
    }

    let msg = timeout(AUTH_TIMEOUT, recv(&mut r)).await??
        .context("client closed before request")?;

    match (role, msg) {
        (Role::Daemon, Msg::Register { name }) => agent(name, r, w, peers).await,
        (Role::Control, Msg::List) => {
            let mut v: Vec<_> = peers.lock().await.keys().cloned().collect();
            v.sort();
            send(&mut w, &Msg::Devices(v)).await
        }
        (Role::Control, Msg::Exec { target, cmd }) => {
            if cmd.len() > MAX_CMD {
                return send(&mut w, &Msg::Error("command too long".into())).await;
            }
            let tx = peers.lock().await.get(&target).cloned();
            let Some(tx) = tx else {
                return send(&mut w, &Msg::Error("target offline".into())).await;
            };
            let (reply, rx) = oneshot::channel();
            if tx.send(Job { cmd, tx: reply }).await.is_err() {
                return send(&mut w, &Msg::Error("target offline".into())).await;
            }
            send(&mut w, &rx.await.unwrap_or_else(|_| Msg::Error("target disconnected".into()))).await
        }
        _ => bail!("request not allowed for this role"),
    }
}

async fn agent<R, W>(
    name: String,
    mut r: BufReader<R>,
    mut w: W,
    peers: Peers,
) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let (tx, mut rx) = mpsc::channel::<Job>(8);
    let inserted = {
        let mut p = peers.lock().await;
        if p.contains_key(&name) { false } else {
            p.insert(name.clone(), tx.clone());
            true
        }
    };
    if !inserted {
        send(&mut w, &Msg::Error("device already online".into())).await?;
        bail!("duplicate device: {name}");
    }
    send(&mut w, &Msg::Ok).await?;
    eprintln!("{name} online");

    let mut tick = interval(Duration::from_secs(10));
    loop {
        tokio::select! {
            Some(job) = rx.recv() => {
                if send(&mut w, &Msg::Run { cmd: job.cmd }).await.is_err() { break }
                let msg = recv(&mut r).await?.unwrap_or_else(|| Msg::Error("target disconnected".into()));
                let _ = job.tx.send(msg);
            }
            _ = tick.tick() => {
                if send(&mut w, &Msg::Ping).await.is_err() { break }
                match timeout(Duration::from_secs(5), recv(&mut r)).await {
                    Ok(Ok(Some(Msg::Pong))) => {}
                    _ => break,
                }
            }
        }
    }

    let mut p = peers.lock().await;
    if p.get(&name).is_some_and(|x| x.same_channel(&tx)) {
        p.remove(&name);
    }
    eprintln!("{name} offline");
    Ok(())
}
