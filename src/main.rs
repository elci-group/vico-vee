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

    init_tracing(&config);

    let state = vico_vee::server::AppState::try_new(config.clone()).await?;

    let app = vico_vee::server::router(state);
    let addr = format!("{}:{}", config.bind, config.port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    tracing::info!(addr = %addr, "vico-vee listening");
    axum::serve(listener, app).await?;
    Ok(())
}

fn init_tracing(config: &Config) {
    let env_filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| "vico_vee=info,tower_http=info".into());

    let registry = tracing_subscriber::registry().with(env_filter);

    match config.log_format {
        LogFormat::Json => {
            registry
                .with(tracing_subscriber::fmt::layer().json())
                .init();
        }
        LogFormat::Compact => {
            registry
                .with(tracing_subscriber::fmt::layer().compact())
                .init();
        }
        LogFormat::Pretty => {
            registry.with(tracing_subscriber::fmt::layer()).init();
        }
    }
}
