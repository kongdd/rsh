mod client;
mod daemon;
mod proto;
mod server;

use anyhow::Result;

#[tokio::main]
async fn main() -> Result<()> {
    let a: Vec<String> = std::env::args().skip(1).collect();
    match a.first().map(String::as_str) {
        Some("server") if a.len() == 1 => server::run().await?,
        Some("daemon") if a.len() == 3 && a[1] == "--name" => daemon::run(&a[2]).await,
        Some("ls") if a.len() == 1 => client::list().await?,
        Some(target) if a.len() > 1 => std::process::exit(client::exec(target, &a[1..].join(" ")).await?),
        _ => eprintln!("rsh server\nrsh daemon --name <name>\nrsh ls\nrsh <target> <cmd>"),
    }
    Ok(())
}
