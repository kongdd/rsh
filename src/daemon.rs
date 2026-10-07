use anyhow::{bail, Result};
use tokio::{io::BufReader, net::TcpStream, process::Command, time::{sleep, Duration}};

use crate::proto::{recv, send, Msg};

pub async fn run(name: &str) {
    let addr = std::env::var("RSH_SERVER").unwrap_or_else(|_| "127.0.0.1:7280".into());
    let token = std::env::var("RSH_TOKEN").expect("RSH_TOKEN is required");
    let mut delay = 1;

    loop {
        match TcpStream::connect(&addr).await {
            Ok(stream) => {
                delay = 1;
                eprintln!("connected as {name}");
                if let Err(e) = session(stream, name, &token).await {
                    eprintln!("disconnected: {e}");
                }
            }
            Err(e) => eprintln!("connect failed: {e}"),
        }
        sleep(Duration::from_secs(delay)).await;
        delay = (delay * 2).min(30);
    }
}

async fn session(stream: TcpStream, name: &str, token: &str) -> Result<()> {
    let (r, mut w) = stream.into_split();
    let mut r = BufReader::new(r);
    send(&mut w, &Msg::Auth { token: token.into() }).await?;
    send(&mut w, &Msg::Register { name: name.into() }).await?;

    while let Some(msg) = recv(&mut r).await? {
        let Msg::Run { cmd } = msg else { bail!("invalid server message") };
        let out = shell(&cmd).await?;
        send(&mut w, &out).await?;
    }
    bail!("server closed connection")
}

async fn shell(cmd: &str) -> Result<Msg> {
    #[cfg(windows)]
    let out = Command::new("cmd").args(["/C", cmd]).output().await?;
    #[cfg(not(windows))]
    let out = Command::new("sh").args(["-lc", cmd]).output().await?;

    Ok(Msg::Result {
        code: out.status.code().unwrap_or(-1),
        out: String::from_utf8_lossy(&out.stdout).into(),
        err: String::from_utf8_lossy(&out.stderr).into(),
    })
}
