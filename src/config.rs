use std::{
    fs::{self, OpenOptions},
    io::{ErrorKind, Write},
    path::PathBuf,
};

use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};

#[derive(Default, Deserialize, Serialize)]
pub struct Config {
    pub server: Option<String>,
    pub bind: Option<String>,
    pub cert: Option<String>,
    pub key: Option<String>,
    pub ca: Option<String>,
    pub server_name: Option<String>,
    pub daemon_token: Option<String>,
    pub control_token: Option<String>,
    pub command_timeout: Option<u64>,
    pub response_timeout: Option<u64>,
    pub nodes: Option<String>,
}

impl Config {
    pub fn load() -> Result<Self> {
        let path = path();
        let Some(path) = path else {
            return Ok(Self::default());
        };
        let text = match fs::read_to_string(&path) {
            Ok(text) => text,
            Err(e) if e.kind() == ErrorKind::NotFound => return Ok(Self::default()),
            Err(e) => return Err(e).with_context(|| path.display().to_string()),
        };
        toml::from_str(&text).with_context(|| path.display().to_string())
    }

    pub fn value(&self, env: &str, value: &Option<String>) -> Option<String> {
        std::env::var(env).ok().or_else(|| value.clone())
    }

    pub fn require(&self, env: &str, value: &Option<String>) -> Result<String> {
        self.value(env, value)
            .with_context(|| format!("{env} is required"))
    }

    pub fn number(&self, env: &str, value: Option<u64>, default: u64) -> Result<u64> {
        match std::env::var(env) {
            Ok(v) => v.parse().with_context(|| format!("invalid {env}")),
            Err(_) => Ok(value.unwrap_or(default)),
        }
    }

    pub fn nodes_path(&self) -> Result<PathBuf> {
        if let Some(path) = self.value("RSH_NODES", &self.nodes) {
            return Ok(path.into());
        }
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(|home| PathBuf::from(home).join(".rsh/nodes.json"))
            .context("nodes path is required")
    }
}

pub fn init() -> Result<()> {
    let path = path().context("config path is required")?;
    let role = prompt("role [server/node/control]")?;
    let mut config = Config::default();
    match role.as_str() {
        "server" => {
            config.cert = Some(prompt("certificate path")?);
            config.key = Some(prompt("private key path")?);
            config.daemon_token = Some(token()?);
            config.control_token = Some(token()?);
            config.nodes = Some(prompt("nodes JSON path")?);
        }
        "node" => {
            config.server = Some(prompt("server address")?);
            config.daemon_token = Some(prompt("node token")?);
        }
        "control" => {
            config.server = Some(prompt("server address")?);
            config.control_token = Some(prompt("control token")?);
        }
        _ => bail!("invalid role"),
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&path)?;
    file.write_all(toml::to_string_pretty(&config)?.as_bytes())?;
    println!("{}", path.display());
    Ok(())
}

pub fn path() -> Option<PathBuf> {
    std::env::var_os("RSH_CONFIG")
        .map(PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(|home| PathBuf::from(home).join(".rsh/config.toml"))
        })
}

fn token() -> Result<String> {
    let mut bytes = [0; 32];
    getrandom::fill(&mut bytes).map_err(|e| anyhow::anyhow!(e.to_string()))?;
    Ok(bytes.iter().map(|x| format!("{x:02x}")).collect())
}

fn prompt(label: &str) -> Result<String> {
    print!("{label}: ");
    std::io::stdout().flush()?;
    let mut value = String::new();
    std::io::stdin().read_line(&mut value)?;
    let value = value.trim().to_string();
    if value.is_empty() {
        bail!("{label} is required");
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::Config;

    #[test]
    fn parses_toml() {
        let config: Config = toml::from_str(
            r#"server = "rsh.example.com:7280"
daemon_token = "secret"
command_timeout = 60"#,
        )
        .unwrap();

        assert_eq!(config.server.as_deref(), Some("rsh.example.com:7280"));
        assert_eq!(config.daemon_token.as_deref(), Some("secret"));
        assert_eq!(config.command_timeout, Some(60));
    }
}
