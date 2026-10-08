use anyhow::{Result, bail};
use tokio::{
    io::{BufReader, split},
    process::Command,
    time::{Duration, sleep, timeout},
};

use crate::{
    config::Config,
    proto::{Msg, Role, recv, send},
    tls,
};

pub async fn run(name: Option<&str>) -> Result<()> {
    let name = match name {
        Some(name) => name.to_string(),
        None => hostname::get()?.to_string_lossy().into_owned(),
    };
    let config = Config::load()?;
    let addr = config.require("RSH_SERVER", &config.server)?;
    let token = config.require("RSH_DAEMON_TOKEN", &config.daemon_token)?;
    let command_timeout = config.number("RSH_COMMAND_TIMEOUT", config.command_timeout, 3600)?;
    let mut add = true;
    let mut delay = 1;

    loop {
        match tls::connect(&addr, &config).await {
            Ok(stream) => {
                delay = 1;
                eprintln!("connected as {name}");
                if let Err(e) = session(stream, &name, &token, command_timeout, &mut add).await {
                    eprintln!("disconnected: {e}");
                }
            }
            Err(e) => eprintln!("connect failed: {e}"),
        }
        sleep(Duration::from_secs(delay)).await;
        delay = (delay * 2).min(30);
    }
}

async fn session(
    stream: tokio_rustls::client::TlsStream<tokio::net::TcpStream>,
    name: &str,
    token: &str,
    command_timeout: u64,
    add: &mut bool,
) -> Result<()> {
    let (r, mut w) = split(stream);
    let mut r = BufReader::new(r);
    send(
        &mut w,
        &Msg::Auth {
            token: token.into(),
            role: Role::Daemon,
        },
    )
    .await?;
    send(
        &mut w,
        &Msg::Register {
            name: name.into(),
            add: *add,
        },
    )
    .await?;

    match recv(&mut r).await? {
        Some(Msg::Ok) => *add = false,
        Some(Msg::Error(e)) => bail!(e),
        _ => bail!("registration failed"),
    }

    while let Some(msg) = recv(&mut r).await? {
        match msg {
            Msg::Ping => send(&mut w, &Msg::Pong).await?,
            Msg::Run { cmd } => send(&mut w, &shell(&cmd, command_timeout).await).await?,
            Msg::RunArgs { args } => send(&mut w, &direct(args, command_timeout).await).await?,
            _ => bail!("invalid server message"),
        }
    }
    bail!("server closed connection")
}

async fn shell(cmd: &str, command_timeout: u64) -> Msg {
    #[cfg(windows)]
    let child = {
        let mut c = Command::new("cmd");
        c.args(["/C", cmd]);
        c
    };
    #[cfg(not(windows))]
    let child = {
        let mut c = Command::new("sh");
        c.args(["-lc", cmd]);
        c
    };

    output(child, command_timeout).await
}

async fn direct(mut args: Vec<String>, command_timeout: u64) -> Msg {
    if args.is_empty() {
        return Msg::Error("empty command".into());
    }
    let mut child = Command::new(args.remove(0));
    child.args(args);
    output(child, command_timeout).await
}

async fn output(mut child: Command, command_timeout: u64) -> Msg {
    child.kill_on_drop(true);
    match timeout(Duration::from_secs(command_timeout), child.output()).await {
        Ok(Ok(out)) => Msg::Result {
            code: out.status.code().unwrap_or(-1),
            out: String::from_utf8_lossy(&out.stdout).into(),
            err: String::from_utf8_lossy(&out.stderr).into(),
        },
        Ok(Err(e)) => Msg::Error(format!("exec failed: {e}")),
        Err(_) => Msg::Result {
            code: 124,
            out: String::new(),
            err: "command timed out\n".into(),
        },
    }
}
