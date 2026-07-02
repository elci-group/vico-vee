//! TLS certificate loading and hot-reload support for `vico-vee`.
//!
//! Provides rustls-backed certificate/key loading with an optional SIGHUP
//! reloader so long-lived deployments can rotate certificates without
//! restarting the process.

use axum::{
    extract::{ConnectInfo, Request, State},
    middleware::{from_fn_with_state, Next},
    response::Response,
};
use rustls::pki_types::CertificateDer;
use rustls::ServerConfig;
use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use tokio_rustls::TlsAcceptor;

/// Paths to a TLS certificate chain and private key.
#[derive(Debug, Clone)]
pub struct TlsConfig {
    pub cert_path: PathBuf,
    pub key_path: PathBuf,
}

impl TlsConfig {
    /// Create a `TlsConfig` from individual paths.
    pub fn new(cert_path: impl Into<PathBuf>, key_path: impl Into<PathBuf>) -> Self {
        Self {
            cert_path: cert_path.into(),
            key_path: key_path.into(),
        }
    }
}

/// Load a rustls `ServerConfig` from PEM-encoded certificate and key files.
pub fn load_rustls_config(cert_path: &Path, key_path: &Path) -> Result<ServerConfig, String> {
    let cert_chain = load_certs(cert_path)?;
    let key = load_private_key(key_path)?;

    let mut config = ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(cert_chain, key)
        .map_err(|e| format!("invalid TLS certificate or key: {e}"))?;

    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    Ok(config)
}

/// Reloadable TLS configuration holder.
///
/// Holds an `Arc<RwLock<ServerConfig>>`. Each call to `acceptor` builds a fresh
/// `TlsAcceptor` from the currently held config, so new connections pick up
/// certificates reloaded via SIGHUP while in-flight connections keep using
/// their original config.
#[derive(Clone)]
pub struct TlsReloader {
    config: Arc<RwLock<ServerConfig>>,
    cert_path: PathBuf,
    key_path: PathBuf,
}

impl TlsReloader {
    /// Load the initial certificate/key pair and create a reloader.
    pub fn new(tls: &TlsConfig) -> Result<Self, String> {
        let config = load_rustls_config(&tls.cert_path, &tls.key_path)?;
        Ok(Self {
            config: Arc::new(RwLock::new(config)),
            cert_path: tls.cert_path.clone(),
            key_path: tls.key_path.clone(),
        })
    }

    /// Return a fresh `TlsAcceptor` backed by the currently held config.
    pub fn acceptor(&self) -> TlsAcceptor {
        let guard = self.config.read().expect("TLS config lock poisoned");
        TlsAcceptor::from(Arc::new(guard.clone()))
    }

    /// Reload certificates from disk immediately.
    pub fn reload(&self) -> Result<(), String> {
        let new_config = load_rustls_config(&self.cert_path, &self.key_path)?;
        let mut guard = self
            .config
            .write()
            .map_err(|_| "TLS config lock poisoned".to_string())?;
        *guard = new_config;
        Ok(())
    }

    /// Spawn a background task that reloads certificates on SIGHUP.
    ///
    /// On non-Unix platforms this is a no-op and returns immediately.
    #[cfg(unix)]
    pub fn spawn_sighup_reloader(self) {
        tokio::spawn(async move {
            let mut stream = match tokio::signal::unix::signal(
                tokio::signal::unix::SignalKind::hangup(),
            ) {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(error = %e, "failed to install SIGHUP handler; cert hot-reload disabled");
                    return;
                }
            };

            loop {
                stream.recv().await;
                match self.reload() {
                    Ok(()) => tracing::info!("TLS certificates reloaded on SIGHUP"),
                    Err(e) => tracing::error!(error = %e, "failed to reload TLS certificates"),
                }
            }
        });
    }

    #[cfg(not(unix))]
    pub fn spawn_sighup_reloader(self) {
        tracing::debug!("SIGHUP cert reload is only supported on Unix");
    }
}

fn load_certs(path: &Path) -> Result<Vec<CertificateDer<'static>>, String> {
    let file = std::fs::File::open(path)
        .map_err(|e| format!("open certificate file {}: {e}", path.display()))?;
    let mut reader = std::io::BufReader::new(file);

    let certs: Vec<_> = rustls_pemfile::certs(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("parse certificate file {}: {e}", path.display()))?;

    Ok(certs.into_iter().map(|c| c.into_owned()).collect())
}

fn load_private_key(path: &Path) -> Result<rustls::pki_types::PrivateKeyDer<'static>, String> {
    let file = std::fs::File::open(path)
        .map_err(|e| format!("open private key file {}: {e}", path.display()))?;
    let mut reader = std::io::BufReader::new(file);

    rustls_pemfile::private_key(&mut reader)
        .map_err(|e| format!("parse private key file {}: {e}", path.display()))?
        .ok_or_else(|| format!("no private key found in {}", path.display()))
}

/// Middleware that injects `ConnectInfo<SocketAddr>` for a single TLS connection.
///
/// axum's built-in server sets this extension automatically for plain HTTP, but
/// our custom HTTPS accept loop must attach it per-connection so that rate
/// limiting and other IP-aware middleware work over TLS.
async fn inject_connect_info(
    State(peer_addr): State<SocketAddr>,
    mut req: Request,
    next: Next,
) -> Response {
    req.extensions_mut().insert(ConnectInfo(peer_addr));
    next.run(req).await
}

/// Serve an axum `Router` over HTTPS using the provided TLS acceptor source.
///
/// Accepts new TLS connections until `shutdown` resolves, then stops
/// accepting. Existing connections are not forcibly closed. A fresh acceptor
/// is fetched for each connection so certificate reloads take effect on new
/// connections.
pub async fn serve_https(
    listener: tokio::net::TcpListener,
    app: axum::Router,
    tls_reloader: TlsReloader,
    shutdown: impl Future<Output = ()>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut shutdown = std::pin::pin!(shutdown);

    loop {
        tokio::select! {
            _ = &mut shutdown => {
                tracing::info!("stopping HTTPS listener");
                break;
            }
            accept = listener.accept() => {
                let (stream, peer_addr) = accept?;
                let app = app.clone();
                let tls_acceptor = tls_reloader.acceptor();
                tokio::spawn(async move {
                    match tls_acceptor.accept(stream).await {
                        Ok(stream) => {
                            let peer_addr = stream
                                .get_ref()
                                .0
                                .peer_addr()
                                .unwrap_or(peer_addr);
                            let io = hyper_util::rt::TokioIo::new(stream);
                            let svc = app
                                .clone()
                                .layer(from_fn_with_state(peer_addr, inject_connect_info));
                            let svc = hyper_util::service::TowerToHyperService::new(svc);
                            let builder = hyper_util::server::conn::auto::Builder::new(
                                hyper_util::rt::TokioExecutor::new(),
                            );
                            let conn = builder.serve_connection_with_upgrades(io, svc);
                            if let Err(e) = conn.await {
                                tracing::debug!(error = %e, "HTTPS connection error");
                            }
                        }
                        Err(e) => tracing::debug!(error = %e, "TLS accept error"),
                    }
                });
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn load_missing_cert_fails() {
        let cert = PathBuf::from("/definitely/not/a/cert.pem");
        let key = PathBuf::from("/definitely/not/a/key.pem");
        assert!(load_rustls_config(&cert, &key).is_err());
    }
}
