use std::{collections::HashMap, sync::Arc};
use anyhow::{bail, Result};
use tokio::{io::BufReader, net::{TcpListener, TcpStream}, sync::{mpsc, oneshot, Mutex}};

use crate::proto::{recv, send, Msg};

type Peers = Arc<Mutex<HashMap<String, mpsc::Sender<Job>>>>;
struct Job { cmd: String, tx: oneshot::Sender<Msg> }

pub async fn run() -> Result<()> {
    let bind = std::env::var("RSH_BIND").unwrap_or_else(|_| "0.0.0.0:7280".into());
    let token = Arc::new(std::env::var("RSH_TOKEN")?);
    let peers: Peers = Default::default();
    let listener = TcpListener::bind(&bind).await?;
    eprintln!("listening on {bind}");

    loop {
        let (s, _) = listener.accept().await?;
        let (peers, token) = (peers.clone(), token.clone());
        tokio::spawn(async move {
            if let Err(e) = handle(s, peers, token).await { eprintln!("{e}") }
        });
    }
}

async fn handle(stream: TcpStream, peers: Peers, token: Arc<String>) -> Result<()> {
    let (r, mut w) = stream.into_split();
    let mut r = BufReader::new(r);

    match recv(&mut r).await? {
        Some(Msg::Auth { token: t }) if t == *token => {}
        _ => bail!("auth failed"),
    }

    match recv(&mut r).await? {
        Some(Msg::Register { name }) => agent(name, r, w, peers).await,
        Some(Msg::List) => {
            let mut v: Vec<_> = peers.lock().await.keys().cloned().collect();
            v.sort();
            send(&mut w, &Msg::Devices(v)).await
        }
        Some(Msg::Exec { target, cmd }) => {
            let tx = peers.lock().await.get(&target).cloned();
            let Some(tx) = tx else { return send(&mut w, &Msg::Error("target offline".into())).await };
            let (reply, rx) = oneshot::channel();
            if tx.send(Job { cmd, tx: reply }).await.is_err() {
                return send(&mut w, &Msg::Error("target offline".into())).await;
            }
            send(&mut w, &rx.await.unwrap_or_else(|_| Msg::Error("target disconnected".into()))).await
        }
        _ => bail!("invalid request"),
    }
}

async fn agent(
    name: String,
    mut r: BufReader<tokio::net::tcp::OwnedReadHalf>,
    mut w: tokio::net::tcp::OwnedWriteHalf,
    peers: Peers,
) -> Result<()> {
    let (tx, mut rx) = mpsc::channel::<Job>(8);
    peers.lock().await.insert(name.clone(), tx.clone());
    eprintln!("{name} online");

    while let Some(job) = rx.recv().await {
        if send(&mut w, &Msg::Run { cmd: job.cmd }).await.is_err() { break }
        let msg = recv(&mut r).await?.unwrap_or_else(|| Msg::Error("target disconnected".into()));
        let _ = job.tx.send(msg);
    }

    let mut p = peers.lock().await;
    if p.get(&name).is_some_and(|x| x.same_channel(&tx)) { p.remove(&name); }
    eprintln!("{name} offline");
    Ok(())
}
