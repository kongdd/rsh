use std::time::SystemTime;

use anyhow::{Result, bail};
use tokio::io::{BufReader, split};

use crate::{
    config::Config,
    proto::{MAX_CMD, Msg, Node, Role, recv, send},
    tls,
};

async fn request(msg: Msg) -> Result<Msg> {
    let config = Config::load()?;
    let server = config.require("RSH_SERVER", &config.server)?;
    let token = config.require("RSH_CONTROL_TOKEN", &config.control_token)?;
    let (r, mut w) = split(tls::connect(&server, &config).await?);
    let mut r = BufReader::new(r);
    send(
        &mut w,
        &Msg::Auth {
            token,
            role: Role::Control,
        },
    )
    .await?;
    send(&mut w, &msg).await?;
    recv(&mut r)
        .await?
        .ok_or_else(|| anyhow::anyhow!("server closed connection"))
}

pub async fn list(json: bool) -> Result<()> {
    show(request(Msg::NodeList).await?, json)
}

pub async fn status(name: &str, json: bool) -> Result<()> {
    show(request(Msg::NodeStatus { name: name.into() }).await?, json)
}

fn show(msg: Msg, json: bool) -> Result<()> {
    match msg {
        Msg::Nodes(nodes) if json => println!("{}", serde_json::to_string_pretty(&nodes)?),
        Msg::Nodes(nodes) => nodes.iter().for_each(print_node),
        Msg::Error(e) => bail!(e),
        _ => bail!("invalid server response"),
    }
    Ok(())
}

fn print_node(node: &Node) {
    let seen = match (node.online, node.last_seen) {
        (true, _) => "-".into(),
        (_, Some(time)) => {
            let now = SystemTime::UNIX_EPOCH
                .elapsed()
                .map_or(time, |x| x.as_secs());
            format!("{}s ago", now.saturating_sub(time))
        }
        _ => "never".into(),
    };
    println!(
        "{}\t{}\t{seen}",
        node.name,
        if node.online { "online" } else { "offline" }
    );
}

pub async fn remove(name: &str) -> Result<()> {
    match request(Msg::NodeRemove { name: name.into() }).await? {
        Msg::Ok => Ok(()),
        Msg::Error(e) => bail!(e),
        _ => bail!("invalid server response"),
    }
}

pub async fn exec(target: &str, cmd: &str) -> Result<i32> {
    if cmd.len() > MAX_CMD {
        bail!("command too long");
    }
    result(
        request(Msg::Exec {
            target: target.into(),
            cmd: cmd.into(),
        })
        .await?,
    )
}

pub async fn exec_args(target: &str, args: &[String]) -> Result<i32> {
    if args.iter().map(String::len).sum::<usize>() > MAX_CMD {
        bail!("command too long");
    }
    result(
        request(Msg::ExecArgs {
            target: target.into(),
            args: args.into(),
        })
        .await?,
    )
}

fn result(msg: Msg) -> Result<i32> {
    match msg {
        Msg::Result { code, out, err } => {
            print!("{out}");
            eprint!("{err}");
            Ok(code)
        }
        Msg::Error(e) => bail!(e),
        _ => bail!("invalid server response"),
    }
}
