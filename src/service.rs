#[cfg(target_os = "linux")]
use std::{fs, process::Command};

#[cfg(target_os = "linux")]
use anyhow::Context;
use anyhow::{Result, bail};

#[cfg(target_os = "linux")]
use crate::config;

#[cfg(target_os = "linux")]
pub fn install(role: &str, name: Option<&str>) -> Result<()> {
    if name.is_some_and(|name| {
        name.is_empty()
            || name.len() > 64
            || !name
                .bytes()
                .all(|c| c.is_ascii_alphanumeric() || b"._-".contains(&c))
    }) {
        bail!("invalid node name");
    }
    let exe = std::env::current_exe()?;
    let config = config::path().context("config path is required")?;
    if !config.exists() {
        bail!("config not found: {}", config.display());
    }
    let (unit, args) = match role {
        "server" if name.is_none() => ("rsh-server", "server".into()),
        "node" => (
            "rsh-node",
            name.map_or_else(
                || "node add".into(),
                |name| format!("node add --name {}", quote(name)),
            ),
        ),
        _ => bail!("invalid service role"),
    };
    let text = format!(
        "[Unit]\nAfter=network-online.target\n\n[Service]\nEnvironment=RSH_CONFIG={}\nExecStart={} {args}\nRestart=always\n\n[Install]\nWantedBy=multi-user.target\n",
        quote(&config.display().to_string()),
        quote(&exe.display().to_string()),
    );
    let path = format!("/etc/systemd/system/{unit}.service");
    fs::write(&path, text).with_context(|| format!("write {path} as root"))?;
    systemctl(&["daemon-reload"])?;
    systemctl(&["enable", "--now", unit])?;
    println!("{unit}.service");
    Ok(())
}

#[cfg(not(target_os = "linux"))]
pub fn install(_: &str, _: Option<&str>) -> Result<()> {
    bail!("systemd is only available on Linux")
}

#[cfg(target_os = "linux")]
fn quote(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

#[cfg(target_os = "linux")]
fn systemctl(args: &[&str]) -> Result<()> {
    let status = Command::new("systemctl").args(args).status()?;
    if !status.success() {
        bail!("systemctl failed");
    }
    Ok(())
}
