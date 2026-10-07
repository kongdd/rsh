use anyhow::{bail, Result};
use tokio::{io::BufReader, net::TcpStream};

use crate::proto::{recv, send, Msg};

async fn connect() -> Result<(BufReader<tokio::net::tcp::OwnedReadHalf>, tokio::net::tcp::OwnedWriteHalf)> {
    let addr = std::env::var("RSH_SERVER").unwrap_or_else(|_| "127.0.0.1:7280".into());
    let token = std::env::var("RSH_TOKEN")?;
    let (r, mut w) = TcpStream::connect(addr).await?.into_split();
    send(&mut w, &Msg::Auth { token }).await?;
    Ok((BufReader::new(r), w))
}

pub async fn list() -> Result<()> {
    let (mut r, mut w) = connect().await?;
    send(&mut w, &Msg::List).await?;
    match recv(&mut r).await? {
        Some(Msg::Devices(v)) => v.iter().for_each(|x| println!("{x}")),
        Some(Msg::Error(e)) => bail!(e),
        _ => bail!("invalid server response"),
    }
    Ok(())
}

pub async fn exec(target: &str, cmd: &str) -> Result<i32> {
    let (mut r, mut w) = connect().await?;
    send(&mut w, &Msg::Exec { target: target.into(), cmd: cmd.into() }).await?;
    match recv(&mut r).await? {
        Some(Msg::Result { code, out, err }) => {
            print!("{out}");
            eprint!("{err}");
            Ok(code)
        }
        Some(Msg::Error(e)) => bail!(e),
        _ => bail!("invalid server response"),
    }
}
