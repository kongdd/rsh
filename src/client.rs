use anyhow::{bail, Result};
use tokio::io::{split, BufReader};

use crate::{
    proto::{recv, send, Msg, Role, MAX_CMD},
    tls,
};

async fn auth<W: tokio::io::AsyncWrite + Unpin>(w: &mut W) -> Result<()> {
    let token = std::env::var("RSH_CONTROL_TOKEN")?;
    send(w, &Msg::Auth { token, role: Role::Control }).await
}

pub async fn list() -> Result<()> {
    let addr = std::env::var("RSH_SERVER").unwrap_or_else(|_| "127.0.0.1:7280".into());
    let (r, mut w) = split(tls::connect(&addr).await?);
    let mut r = BufReader::new(r);
    auth(&mut w).await?;
    send(&mut w, &Msg::List).await?;
    match recv(&mut r).await? {
        Some(Msg::Devices(v)) => v.iter().for_each(|x| println!("{x}")),
        Some(Msg::Error(e)) => bail!(e),
        _ => bail!("invalid server response"),
    }
    Ok(())
}

pub async fn exec(target: &str, cmd: &str) -> Result<i32> {
    if cmd.len() > MAX_CMD {
        bail!("command too long");
    }
    let addr = std::env::var("RSH_SERVER").unwrap_or_else(|_| "127.0.0.1:7280".into());
    let (r, mut w) = split(tls::connect(&addr).await?);
    let mut r = BufReader::new(r);
    auth(&mut w).await?;
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
