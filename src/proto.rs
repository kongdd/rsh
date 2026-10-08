use anyhow::{Result, bail};
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
pub struct Node {
    pub name: String,
    pub online: bool,
    pub last_seen: Option<u64>,
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Msg {
    Auth { token: String, role: Role },
    Register { name: String, add: bool },
    Exec { target: String, cmd: String },
    ExecArgs { target: String, args: Vec<String> },
    Run { cmd: String },
    RunArgs { args: Vec<String> },
    Result { code: i32, out: String, err: String },
    NodeRemove { name: String },
    NodeList,
    NodeStatus { name: String },
    Nodes(Vec<Node>),
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
            if data.is_empty() {
                return Ok(None);
            }
            bail!("incomplete message");
        }
        let n = buf
            .iter()
            .position(|&b| b == b'\n')
            .map_or(buf.len(), |i| i + 1);
        if data.len() + n > MAX_MSG {
            bail!("message too large");
        }
        data.extend_from_slice(&buf[..n]);
        r.consume(n);
        if data.last() == Some(&b'\n') {
            break;
        }
    }
    Ok(Some(serde_json::from_slice(&data)?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncWriteExt, BufReader};

    #[tokio::test]
    async fn round_trip() {
        let (mut tx, rx) = tokio::io::duplex(128);
        send(&mut tx, &Msg::Ping).await.unwrap();
        assert!(matches!(
            recv(&mut BufReader::new(rx)).await,
            Ok(Some(Msg::Ping))
        ));
    }

    #[tokio::test]
    async fn rejects_incomplete_message() {
        let (mut tx, rx) = tokio::io::duplex(128);
        tx.write_all(br#"{"Ping":null}"#).await.unwrap();
        drop(tx);
        assert!(recv(&mut BufReader::new(rx)).await.is_err());
    }

    #[tokio::test]
    async fn rejects_oversized_message() {
        let data = vec![b'x'; MAX_MSG + 1];
        assert!(recv(&mut BufReader::new(data.as_slice())).await.is_err());
    }
}
