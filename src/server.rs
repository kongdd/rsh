use std::{collections::HashMap, sync::Arc};

use anyhow::{Context, Result, bail};
use tokio::{
    io::{AsyncRead, AsyncWrite, BufReader, split},
    net::TcpListener,
    sync::{Mutex, mpsc, oneshot},
    time::{Duration, interval, timeout},
};

use crate::{
    config::Config,
    nodes::Nodes,
    proto::{MAX_CMD, Msg, Node, Role, recv, send},
    tls,
};

const AUTH_TIMEOUT: Duration = Duration::from_secs(5);
const RESPONSE_TIMEOUT: u64 = 3660;

type Peers = Arc<Mutex<HashMap<String, mpsc::Sender<AgentMsg>>>>;
type Registry = Arc<Mutex<Nodes>>;

enum AgentMsg {
    Run(Job),
    Stop,
}

enum Command {
    Shell(String),
    Args(Vec<String>),
}

struct Job {
    command: Command,
    tx: oneshot::Sender<Msg>,
}

struct Tokens {
    daemon: String,
    control: String,
}

pub async fn run() -> Result<()> {
    let config = Config::load()?;
    let bind = config
        .value("RSH_BIND", &config.bind)
        .unwrap_or_else(|| "0.0.0.0:7280".into());
    let tokens = Arc::new(Tokens {
        daemon: config.require("RSH_DAEMON_TOKEN", &config.daemon_token)?,
        control: config.require("RSH_CONTROL_TOKEN", &config.control_token)?,
    });
    let response_timeout = config.number(
        "RSH_RESPONSE_TIMEOUT",
        config.response_timeout,
        RESPONSE_TIMEOUT,
    )?;
    let acceptor = tls::acceptor(&config)?;
    let nodes = Arc::new(Mutex::new(Nodes::load(config.nodes_path()?)?));
    let peers: Peers = Default::default();
    let listener = TcpListener::bind(&bind).await?;
    eprintln!("listening with TLS on {bind}");

    loop {
        let (tcp, _) = listener.accept().await?;
        let (peers, nodes, tokens, acceptor) = (
            peers.clone(),
            nodes.clone(),
            tokens.clone(),
            acceptor.clone(),
        );
        tokio::spawn(async move {
            let result = async {
                let stream = timeout(AUTH_TIMEOUT, acceptor.accept(tcp)).await??;
                handle(stream, peers, nodes, tokens, response_timeout).await
            }
            .await;
            if let Err(e) = result {
                eprintln!("{e}")
            }
        });
    }
}

async fn handle<S>(
    stream: S,
    peers: Peers,
    nodes: Registry,
    tokens: Arc<Tokens>,
    response_timeout: u64,
) -> Result<()>
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

    let msg = timeout(AUTH_TIMEOUT, recv(&mut r))
        .await??
        .context("client closed before request")?;

    match (role, msg) {
        (Role::Daemon, Msg::Register { name, add }) => {
            if !valid_name(&name) {
                return send(&mut w, &Msg::Error("invalid node name".into())).await;
            }
            let registered = {
                let mut nodes = nodes.lock().await;
                if add {
                    nodes.add(name.clone())?;
                    true
                } else {
                    nodes.contains(&name)
                }
            };
            if !registered {
                send(&mut w, &Msg::Error("node not registered".into())).await?;
                bail!("unregistered node: {name}");
            }
            agent(name, r, w, peers, nodes, response_timeout).await
        }
        (Role::Control, Msg::NodeList) => {
            let list = {
                let peers = peers.lock().await;
                let nodes = nodes.lock().await;
                nodes
                    .list()
                    .map(|(name, last_seen)| Node {
                        name: name.clone(),
                        online: peers.contains_key(name),
                        last_seen,
                    })
                    .collect()
            };
            send(&mut w, &Msg::Nodes(list)).await
        }
        (Role::Control, Msg::NodeStatus { name }) => {
            let node = {
                let peers = peers.lock().await;
                let nodes = nodes.lock().await;
                nodes
                    .list()
                    .find(|(node, _)| *node == &name)
                    .map(|(name, last_seen)| Node {
                        name: name.clone(),
                        online: peers.contains_key(name),
                        last_seen,
                    })
            };
            match node {
                Some(node) => send(&mut w, &Msg::Nodes(vec![node])).await,
                None => send(&mut w, &Msg::Error("node not found".into())).await,
            }
        }
        (Role::Control, Msg::NodeRemove { name }) => {
            if !nodes.lock().await.remove(&name)? {
                return send(&mut w, &Msg::Error("node not found".into())).await;
            }
            let tx = peers.lock().await.remove(&name);
            if let Some(tx) = tx {
                let _ = tx.send(AgentMsg::Stop).await;
            }
            send(&mut w, &Msg::Ok).await
        }
        (Role::Control, Msg::Exec { target, cmd }) => {
            dispatch(&mut w, &peers, &target, Command::Shell(cmd)).await
        }
        (Role::Control, Msg::ExecArgs { target, args }) => {
            dispatch(&mut w, &peers, &target, Command::Args(args)).await
        }
        _ => bail!("request not allowed for this role"),
    }
}

fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 64
        && name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
}

async fn dispatch<W>(w: &mut W, peers: &Peers, target: &str, command: Command) -> Result<()>
where
    W: AsyncWrite + Unpin,
{
    let len = match &command {
        Command::Shell(cmd) => cmd.len(),
        Command::Args(args) => args.iter().map(String::len).sum(),
    };
    if len > MAX_CMD {
        return send(w, &Msg::Error("command too long".into())).await;
    }
    let Some(tx) = peers.lock().await.get(target).cloned() else {
        return send(w, &Msg::Error("target offline".into())).await;
    };
    let (reply, rx) = oneshot::channel();
    if tx
        .send(AgentMsg::Run(Job { command, tx: reply }))
        .await
        .is_err()
    {
        return send(w, &Msg::Error("target offline".into())).await;
    }
    send(
        w,
        &rx.await
            .unwrap_or_else(|_| Msg::Error("target disconnected".into())),
    )
    .await
}

async fn agent<R, W>(
    name: String,
    mut r: BufReader<R>,
    mut w: W,
    peers: Peers,
    nodes: Registry,
    response_timeout: u64,
) -> Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let (tx, mut rx) = mpsc::channel::<AgentMsg>(8);
    let inserted = {
        let mut p = peers.lock().await;
        if p.contains_key(&name) {
            false
        } else {
            p.insert(name.clone(), tx.clone());
            true
        }
    };
    if !inserted {
        send(&mut w, &Msg::Error("device already online".into())).await?;
        bail!("duplicate device: {name}");
    }
    let result = async {
        send(&mut w, &Msg::Ok).await?;
        eprintln!("{name} online");
        let mut tick = interval(Duration::from_secs(10));
        loop {
            tokio::select! {
                msg = rx.recv() => match msg {
                    Some(AgentMsg::Run(job)) => {
                        let msg = match job.command {
                            Command::Shell(cmd) => Msg::Run { cmd },
                            Command::Args(args) => Msg::RunArgs { args },
                        };
                        if send(&mut w, &msg).await.is_err() {
                            break;
                        }
                        let msg = match timeout(Duration::from_secs(response_timeout), recv(&mut r)).await {
                            Ok(Ok(Some(msg @ (Msg::Result { .. } | Msg::Error(_))))) => msg,
                            Ok(Ok(Some(_))) => bail!("invalid command response"),
                            Ok(Ok(None)) => bail!("target disconnected"),
                            Ok(Err(e)) => return Err(e),
                            Err(_) => bail!("command response timed out"),
                        };
                        let _ = job.tx.send(msg);
                    }
                    Some(AgentMsg::Stop) | None => break,
                },
                _ = tick.tick() => {
                    if send(&mut w, &Msg::Ping).await.is_err() {
                        break;
                    }
                    match timeout(Duration::from_secs(5), recv(&mut r)).await {
                        Ok(Ok(Some(Msg::Pong))) => {}
                        _ => break,
                    }
                }
            }
        }
        Ok(())
    }
    .await;

    let mut p = peers.lock().await;
    if p.get(&name).is_some_and(|x| x.same_channel(&tx)) {
        p.remove(&name);
    }
    drop(p);
    nodes.lock().await.seen(&name)?;
    eprintln!("{name} offline");
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn removes_peer_after_invalid_response() {
        let peers: Peers = Default::default();
        let path = std::env::temp_dir().join(format!("rsh-server-{}.json", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let mut registry = Nodes::load(path.clone()).unwrap();
        registry.add("node".into()).unwrap();
        let nodes = Arc::new(Mutex::new(registry));
        let (server, client) = tokio::io::duplex(1024);
        let (server_r, server_w) = split(server);
        let (client_r, mut client_w) = split(client);
        let mut client_r = BufReader::new(client_r);
        let task = tokio::spawn(agent(
            "node".into(),
            BufReader::new(server_r),
            server_w,
            peers.clone(),
            nodes,
            RESPONSE_TIMEOUT,
        ));

        assert!(matches!(recv(&mut client_r).await, Ok(Some(Msg::Ok))));
        let tx = peers.lock().await["node"].clone();
        let (reply, rx) = oneshot::channel();
        tx.send(AgentMsg::Run(Job {
            command: Command::Shell("true".into()),
            tx: reply,
        }))
        .await
        .unwrap();

        loop {
            match recv(&mut client_r).await.unwrap().unwrap() {
                Msg::Ping => send(&mut client_w, &Msg::Pong).await.unwrap(),
                Msg::Run { .. } => {
                    send(&mut client_w, &Msg::Pong).await.unwrap();
                    break;
                }
                msg => panic!("unexpected message: {msg:?}"),
            }
        }

        assert!(rx.await.is_err());
        assert!(task.await.unwrap().is_err());
        assert!(!peers.lock().await.contains_key("node"));
        std::fs::remove_file(path).unwrap();
    }
}
