//! TLS certificate loading and hot-reload support for `vico-vee`.
//!
//! Provides rustls-backed certificate/key loading with an optional SIGHUP
//! reloader so long-lived deployments can rotate certificates without
//! restarting the process.

use rustls::pki_types::{CertificateDer, PrivateKeyDer};
use rustls::ServerConfig;
use std::future::Future;
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
/// Holds an `Arc<RwLock<ServerConfig>>` so the underlying rustls config can
/// be replaced on SIGHUP while active connections continue using their
/// previously accepted config.
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

    /// Return a `TlsAcceptor` backed by the currently held config.
    pub fn acceptor(&self) -> TlsAcceptor {
        TlsAcceptor::from(self.config.clone())
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

fn load_private_key(path: &Path) -> Result<PrivateKeyDer<'static>, String> {
    let file = std::fs::File::open(path)
        .map_err(|e| format!("open private key file {}: {e}", path.display()))?;
    let mut reader = std::io::BufReader::new(file);

    // Attempt to read a PKCS#8 key first, then fall back to RSA PKCS#1.
    let mut keys = rustls_pemfile::pkcs8_private_keys(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("parse private key file {}: {e}", path.display()))?;

    if let Some(key) = keys.pop() {
        return Ok(key.into_owned().into());
    }

    let file = std::fs::File::open(path)
        .map_err(|e| format!("re-open private key file {}: {e}", path.display()))?;
    let mut reader = std::io::BufReader::new(file);
    keys = rustls_pemfile::rsa_private_keys(&mut reader)
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| format!("parse RSA private key file {}: {e}", path.display()))?;

    keys.pop()
        .map(|k| k.into_owned().into())
        .ok_or_else(|| format!("no private key found in {}", path.display()))
}

/// Serve an axum `Router` over HTTPS using the provided TLS acceptor.
///
/// Accepts new TLS connections until `shutdown` resolves, then stops
/// accepting. Existing connections are not forcibly closed.
pub async fn serve_https(
    listener: tokio::net::TcpListener,
    app: axum::Router,
    tls_acceptor: TlsAcceptor,
    shutdown: impl Future<Output = ()>,
) -> Result<(), Box<dyn std::error::Error>> {
    let app = Arc::new(app);
    let mut shutdown = std::pin::pin!(shutdown);

    loop {
        tokio::select! {
            _ = &mut shutdown => {
                tracing::info("stopping HTTPS listener");
                break;
            }
            accept = listener.accept() => {
                let (stream, _) = accept?;
                let app = app.clone();
                let tls_acceptor = tls_acceptor.clone();
                tokio::spawn(async move {
                    match tls_acceptor.accept(stream).await {
                        Ok(stream) => {
                            let io = hyper_util::rt::TokioIo::new(stream);
                            let svc = app.clone();
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
