use anyhow::Result;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

#[derive(Debug, Serialize, Deserialize)]
pub enum Msg {
    Auth { token: String },
    Register { name: String },
    Exec { target: String, cmd: String },
    Run { cmd: String },
    Result { code: i32, out: String, err: String },
    List,
    Devices(Vec<String>),
    Ping,
    Pong,
    Error(String),
}

pub async fn send<W: AsyncWrite + Unpin>(w: &mut W, msg: &Msg) -> Result<()> {
    let mut data = serde_json::to_vec(msg)?;
    data.push(b'\n');
    w.write_all(&data).await?;
    Ok(())
}

pub async fn recv<R: AsyncBufRead + Unpin>(r: &mut R) -> Result<Option<Msg>> {
    let mut line = String::new();
    if r.read_line(&mut line).await? == 0 {
        return Ok(None);
    }
    Ok(Some(serde_json::from_str(&line)?))
}
