use std::{fs::File, io::BufReader, sync::Arc};

use anyhow::{Context, Result, anyhow};
use rustls_pki_types::ServerName;
use tokio::{
    net::TcpStream,
    time::{Duration, timeout},
};
use tokio_rustls::{
    TlsAcceptor, TlsConnector,
    client::TlsStream,
    rustls::{ClientConfig, RootCertStore, ServerConfig},
};

use crate::config::Config;

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

pub fn acceptor(config: &Config) -> Result<TlsAcceptor> {
    let cert = config.require("RSH_CERT", &config.cert)?;
    let key = config.require("RSH_KEY", &config.key)?;
    let certs = rustls_pemfile::certs(&mut BufReader::new(File::open(cert)?))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let key = rustls_pemfile::private_key(&mut BufReader::new(File::open(key)?))?
        .context("no private key found")?;
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}

pub async fn connect(addr: &str, settings: &Config) -> Result<TlsStream<TcpStream>> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    if let Some(path) = settings.value("RSH_CA", &settings.ca) {
        for cert in rustls_pemfile::certs(&mut BufReader::new(File::open(path)?)) {
            roots.add(cert?)?;
        }
    }

    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(config));
    let host = settings
        .value("RSH_SERVER_NAME", &settings.server_name)
        .unwrap_or_else(|| host(addr));
    let name =
        ServerName::try_from(host.clone()).map_err(|_| anyhow!("invalid server name: {host}"))?;
    let tcp = timeout(CONNECT_TIMEOUT, TcpStream::connect(addr)).await??;
    Ok(timeout(CONNECT_TIMEOUT, connector.connect(name, tcp)).await??)
}

fn host(addr: &str) -> String {
    if let Some(rest) = addr.strip_prefix('[') {
        return rest.split(']').next().unwrap_or(addr).to_string();
    }
    addr.rsplit_once(':')
        .map_or(addr, |(host, _)| host)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::host;

    #[test]
    fn extracts_host() {
        assert_eq!(host("example.com:7280"), "example.com");
        assert_eq!(host("[::1]:7280"), "::1");
        assert_eq!(host("localhost"), "localhost");
    }
}
