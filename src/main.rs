//! Standalone `vico-vee` service binary.
//!
//! Starts the ViCo Execution Environment as a separate HTTP server that ViCo
//! (or any other client) can reach over a well-defined REST API.

use clap::Parser;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use vico_vee::config::{Cli, Config, LogFormat};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
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

    let app = vico_vee::server::router(state);
    let listener = tokio::net::TcpListener::bind((config.bind.as_str(), config.port)).await?;

    let shutdown = async move {
        let _ = tokio::signal::ctrl_c().await;
    };

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
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown)
            .await?;
    }

    Ok(())
}
