use std::{fs::File, io::BufReader, sync::Arc};

use anyhow::{anyhow, Context, Result};
use rustls_pki_types::ServerName;
use tokio::{net::TcpStream, time::{timeout, Duration}};
use tokio_rustls::{
    client::TlsStream,
    rustls::{ClientConfig, RootCertStore, ServerConfig},
    TlsAcceptor, TlsConnector,
};

const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

pub fn acceptor() -> Result<TlsAcceptor> {
    let cert = std::env::var("RSH_CERT").context("RSH_CERT is required")?;
    let key = std::env::var("RSH_KEY").context("RSH_KEY is required")?;
    let certs = rustls_pemfile::certs(&mut BufReader::new(File::open(cert)?))
        .collect::<std::result::Result<Vec<_>, _>>()?;
    let key = rustls_pemfile::private_key(&mut BufReader::new(File::open(key)?))?
        .context("no private key found")?;
    let config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(certs, key)?;
    Ok(TlsAcceptor::from(Arc::new(config)))
}

pub async fn connect(addr: &str) -> Result<TlsStream<TcpStream>> {
    let mut roots = RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());

    if let Ok(path) = std::env::var("RSH_CA") {
        for cert in rustls_pemfile::certs(&mut BufReader::new(File::open(path)?)) {
            roots.add(cert?)?;
        }
    }

    let config = ClientConfig::builder()
        .with_root_certificates(roots)
        .with_no_client_auth();
    let connector = TlsConnector::from(Arc::new(config));
    let host = std::env::var("RSH_SERVER_NAME").unwrap_or_else(|_| host(addr));
    let name = ServerName::try_from(host.clone()).map_err(|_| anyhow!("invalid server name: {host}"))?;
    let tcp = timeout(CONNECT_TIMEOUT, TcpStream::connect(addr)).await??;
    Ok(timeout(CONNECT_TIMEOUT, connector.connect(name, tcp)).await??)
}

fn host(addr: &str) -> String {
    if let Some(rest) = addr.strip_prefix('[') {
        return rest.split(']').next().unwrap_or(addr).to_string();
    }
    addr.rsplit_once(':').map_or(addr, |(host, _)| host).to_string()
}
