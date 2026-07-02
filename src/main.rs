//! Standalone `vico-vee` service binary.
//!
//! Starts the ViCo Execution Environment as a separate HTTP server that ViCo
//! (or any other client) can reach over a well-defined REST API.

use clap::Parser;
use std::path::PathBuf;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use vico_vee::config::{Cli, Config, LogFormat};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();

    // CLI-only commands: run and exit.
    if let Some(maybe_path) = cli.generate_admin_key {
        let target = maybe_path.unwrap_or_else(|| {
            cli.config_dir
                .clone()
                .or_else(|| std::env::var("VICO_VEE_CONFIG_DIR").ok().map(PathBuf::from))
                .unwrap_or_else(vico_vee::paths::vee_config_dir)
                .join("api_keys.toml")
        });
        let token = generate_admin_key(&target)?;
        println!("generated admin API key: {token}");
        println!("wrote {target}", target = target.display());
        return Ok(());
    }

    let config = Config::load(Some(cli.clone()))?;

    // CLI-only commands: run and exit.
    if cli.backup {
        let path = vico_vee::backup::run_backup(&config, cli.backup_output)?;
        println!("backup created: {}", path.display());
        return Ok(());
    }
    if let Some(input) = cli.restore {
        vico_vee::backup::run_restore(&config, input)?;
        println!("restore completed");
        return Ok(());
    }

    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "vico_vee=info,tower_http=info".into());

    match config.log_format {
        LogFormat::Json => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(tracing_subscriber::fmt::layer().json())
                .init();
        }
        LogFormat::Pretty => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(tracing_subscriber::fmt::layer())
                .init();
        }
        LogFormat::Compact => {
            tracing_subscriber::registry()
                .with(env_filter)
                .with(tracing_subscriber::fmt::layer().compact())
                .init();
        }
    }

    let state = vico_vee::server::AppState::try_new(config.clone()).await?;
    let shutdown_state = state.clone();

    let app = vico_vee::server::router(state);
    let listener = tokio::net::TcpListener::bind((config.bind.as_str(), config.port)).await?;

    let shutdown = shutdown_signal();

    if let (Some(cert_path), Some(key_path)) = (&config.tls_cert, &config.tls_key) {
        tracing::info!(
            bind = %config.bind,
            port = config.port,
            cert = %cert_path.display(),
            key = %key_path.display(),
            "vico-vee listening with TLS"
        );
        let tls_config = vico_vee::tls::TlsConfig::new(cert_path, key_path);
        let tls_reloader = vico_vee::tls::TlsReloader::new(&tls_config)?;
        tls_reloader.clone().spawn_sighup_reloader();
        vico_vee::tls::serve_https(listener, app, tls_reloader, shutdown).await?;
    } else {
        tracing::info!(bind = %config.bind, port = config.port, "vico-vee listening");
        axum::serve(
            listener,
            app.into_make_service_with_connect_info::<std::net::SocketAddr>(),
        )
        .with_graceful_shutdown(shutdown)
        .await?;
    }

    // Stop accepting new requests and wait for in-flight executions to finish.
    tracing::info!(
        grace_period = config.shutdown_grace_period_secs,
        "graceful shutdown initiated"
    );
    let timeout = std::time::Duration::from_secs(config.shutdown_grace_period_secs);
    if !shutdown_state.vee.wait_for_inflight(timeout).await {
        tracing::warn!("shutdown grace period expired with in-flight executions remaining");
    }
    shutdown_state.vee.stop().await;
    tracing::info!("vico-vee shutdown complete");

    Ok(())
}

/// Wait for SIGINT (Ctrl-C) or SIGTERM, whichever arrives first.
async fn shutdown_signal() {
    #[cfg(unix)]
    {
        let mut sigint = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
            .expect("failed to install SIGINT handler");
        let mut sigterm = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
            .expect("failed to install SIGTERM handler");
        tokio::select! {
            _ = sigint.recv() => tracing::info!("received SIGINT, shutting down"),
            _ = sigterm.recv() => tracing::info!("received SIGTERM, shutting down"),
        }
    }
    #[cfg(not(unix))]
    {
        let _ = tokio::signal::ctrl_c().await;
        tracing::info!("received Ctrl-C, shutting down");
    }
}

/// Generate a fresh admin API key and write it to `path` in the TOML format
/// expected by [`vico_vee::auth::AuthKeys`].
fn generate_admin_key(path: &PathBuf) -> Result<String, Box<dyn std::error::Error>> {
    use rand::RngCore;

    let mut bytes = [0u8; 32];
    rand::rng().fill_bytes(&mut bytes);
    let token = hex::encode(bytes);

    let contents = format!(
        "# vico-vee API keys\n# Generated by --generate-admin-key\n[keys.admin]\ntoken = \"{token}\"\nscopes = [\"submit\", \"read\", \"admin\"]\n"
    );

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, contents)?;

    Ok(token)
}
