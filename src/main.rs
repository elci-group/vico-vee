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
    let config = Config::load(Some(cli))?;

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

    tracing::info!(bind = %config.bind, port = config.port, "vico-vee listening");
    axum::serve(listener, app).await?;
    Ok(())
}
