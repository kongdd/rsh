use anyhow::{bail, Result};
use tokio::{
    io::{split, BufReader},
    process::Command,
    time::{sleep, timeout, Duration},
};

use crate::{
    proto::{recv, send, Msg, Role},
    tls,
};

pub async fn run(name: Option<&str>) -> Result<()> {
    let name = match name {
        Some(name) => name.to_string(),
        None => hostname::get()?.to_string_lossy().into_owned(),
    };
    let addr = std::env::var("RSH_SERVER").unwrap_or_else(|_| "127.0.0.1:7280".into());
    let token = std::env::var("RSH_DAEMON_TOKEN")?;
    let mut delay = 1;

    loop {
        match tls::connect(&addr).await {
            Ok(stream) => {
                delay = 1;
                eprintln!("connected as {name}");
                if let Err(e) = session(stream, &name, &token).await {
                    eprintln!("disconnected: {e}");
                }
            }
            Err(e) => eprintln!("connect failed: {e}"),
        }
        sleep(Duration::from_secs(delay)).await;
        delay = (delay * 2).min(30);
    }
}

async fn session(stream: tokio_rustls::client::TlsStream<tokio::net::TcpStream>, name: &str, token: &str) -> Result<()> {
    let (r, mut w) = split(stream);
    let mut r = BufReader::new(r);
    send(&mut w, &Msg::Auth { token: token.into(), role: Role::Daemon }).await?;
    send(&mut w, &Msg::Register { name: name.into() }).await?;

    match recv(&mut r).await? {
        Some(Msg::Ok) => {}
        Some(Msg::Error(e)) => bail!(e),
        _ => bail!("registration failed"),
    }

    while let Some(msg) = recv(&mut r).await? {
        match msg {
            Msg::Ping => send(&mut w, &Msg::Pong).await?,
            Msg::Run { cmd } => send(&mut w, &shell(&cmd).await).await?,
            _ => bail!("invalid server message"),
        }
    }
    bail!("server closed connection")
}

async fn shell(cmd: &str) -> Msg {
    #[cfg(windows)]
    let mut child = {
        let mut c = Command::new("cmd");
        c.args(["/C", cmd]);
        c
    };
    #[cfg(not(windows))]
    let mut child = {
        let mut c = Command::new("sh");
        c.args(["-lc", cmd]);
        c
    };

    child.kill_on_drop(true);
    let secs = std::env::var("RSH_COMMAND_TIMEOUT").ok()
        .and_then(|v| v.parse().ok()).unwrap_or(3600);

    match timeout(Duration::from_secs(secs), child.output()).await {
        Ok(Ok(out)) => Msg::Result {
            code: out.status.code().unwrap_or(-1),
            out: String::from_utf8_lossy(&out.stdout).into(),
            err: String::from_utf8_lossy(&out.stderr).into(),
        },
        Ok(Err(e)) => Msg::Error(format!("exec failed: {e}")),
        Err(_) => Msg::Result { code: 124, out: String::new(), err: "command timed out\n".into() },
    }
}
