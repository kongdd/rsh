use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

pub const MAX_MSG: usize = 8 * 1024 * 1024;
pub const MAX_CMD: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum Role {
    Daemon,
    Control,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Msg {
    Auth { token: String, role: Role },
    Register { name: String },
    Exec { target: String, cmd: String },
    Run { cmd: String },
    Result { code: i32, out: String, err: String },
    List,
    Devices(Vec<String>),
    Ping,
    Pong,
    Ok,
    Error(String),
}

pub async fn send<W: AsyncWrite + Unpin>(w: &mut W, msg: &Msg) -> Result<()> {
    let data = serde_json::to_vec(msg)?;
    if data.len() + 1 > MAX_MSG {
        bail!("message too large");
    }
    w.write_all(&data).await?;
    w.write_all(b"\n").await?;
    Ok(())
}

pub async fn recv<R: AsyncBufRead + Unpin>(r: &mut R) -> Result<Option<Msg>> {
    let mut data = Vec::new();
    loop {
        let buf = r.fill_buf().await?;
        if buf.is_empty() {
            if data.is_empty() { return Ok(None) }
            bail!("incomplete message");
        }
        let n = buf.iter().position(|&b| b == b'\n').map_or(buf.len(), |i| i + 1);
        if data.len() + n > MAX_MSG {
            bail!("message too large");
        }
        data.extend_from_slice(&buf[..n]);
        r.consume(n);
        if data.last() == Some(&b'\n') { break }
    }
    Ok(Some(serde_json::from_slice(&data)?))
}
