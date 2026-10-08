mod client;
mod config;
mod daemon;
mod nodes;
mod proto;
mod server;
mod service;
mod tls;

use anyhow::Result;

const USAGE: &str = "rsh server
rsh node add [--name <name>]
rsh node rm <name>
rsh node list [--json]
rsh node status <name> [--json]
rsh run <target> <cmd>
rsh run <target> -- <program> [args...]
rsh config init
rsh service install <server|node> [--name <name>]";

#[tokio::main]
async fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    match a.first().map(String::as_str) {
        Some("-h" | "--help" | "help") if a.len() == 1 => println!("{USAGE}"),
        Some("server") if a.len() == 1 => server::run().await?,
        Some("node") if a.len() == 2 && a[1] == "add" => daemon::run(None).await?,
        Some("node") if a.len() == 4 && a[1] == "add" && a[2] == "--name" => {
            daemon::run(Some(&a[3])).await?
        }
        Some("node") if a.len() == 3 && a[1] == "rm" => client::remove(&a[2]).await?,
        Some("node") if a.len() == 2 && a[1] == "list" => client::list(false).await?,
        Some("node") if a.len() == 3 && a[1] == "list" && a[2] == "--json" => {
            client::list(true).await?
        }
        Some("node") if a.len() == 3 && a[1] == "status" => client::status(&a[2], false).await?,
        Some("node") if a.len() == 4 && a[1] == "status" && a[3] == "--json" => {
            client::status(&a[2], true).await?
        }
        Some("run") if a.len() > 3 && a[2] == "--" => {
            std::process::exit(client::exec_args(&a[1], &a[3..]).await?)
        }
        Some("run") if a.len() > 2 => {
            std::process::exit(client::exec(&a[1], &a[2..].join(" ")).await?)
        }
        Some("config") if a.len() == 2 && a[1] == "init" => config::init()?,
        Some("service") if a.len() == 3 && a[1] == "install" => service::install(&a[2], None)?,
        Some("service")
            if a.len() == 5 && a[1] == "install" && a[2] == "node" && a[3] == "--name" =>
        {
            service::install("node", Some(&a[4]))?
        }
        _ => invalid(),
    }
    Ok(())
}

fn invalid() -> ! {
    eprintln!("{USAGE}");
    std::process::exit(2)
}
