//! Standalone `vico-vee` service binary.
//!
//! Starts the ViCo Execution Environment as a separate HTTP server that ViCo
//! (or any other client) can reach over a well-defined REST API.

use clap::Parser;
use std::time::Duration;
use tokio::signal;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};
use vico_vee::config::{Cli, Command, Config, LogFormat};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    match &cli.command {
        Some(Command::Backup { output }) => {
            let config = Config::load(Some(cli.clone()))?;
            let path = vico_vee::backup::run_backup(&config, output.clone())?;
            println!("Backup created: {}", path.display());
            return Ok(());
        }
        Some(Command::Restore { input }) => {
            let config = Config::load(Some(cli.clone()))?;
            vico_vee::backup::run_restore(&config, input.clone())?;
            println!("Restore completed.");
            return Ok(());
        }
        _ => {}
    }
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

    let app = vico_vee::server::router(state.clone());
    let listener = tokio::net::TcpListener::bind((config.bind.as_str(), config.port)).await?;

    let use_tls = config.tls_cert.is_some() && config.tls_key.is_some();

    let shutdown_token = tokio_util::sync::CancellationToken::new();
    let shutdown_clone = shutdown_token.clone();
    tokio::spawn(async move {
        shutdown_signal().await;
        shutdown_clone.cancel();
    });

    if use_tls {
        let cert_path = config.tls_cert.as_ref().unwrap();
        let key_path = config.tls_key.as_ref().unwrap();
        let tls_config = vico_vee::tls::TlsConfig::new(cert_path, key_path);
        let tls_reloader = vico_vee::tls::TlsReloader::new(&tls_config)?;
        tls_reloader.clone().spawn_sighup_reloader();

        tracing::info!(
            bind = %config.bind,
            port = config.port,
            tls = true,
            "vico-vee listening"
        );

        vico_vee::tls::serve_https(
            listener,
            app,
            tls_reloader,
            shutdown_token.cancelled_owned(),
        )
        .await?;
    } else {
        tracing::warn!(
            "TLS is not configured; vico-vee will serve plain HTTP. Set --tls-cert and --tls-key to enable HTTPS."
        );

        tracing::info!(
            bind = %config.bind,
            port = config.port,
            tls = false,
            "vico-vee listening"
        );

        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_token.cancelled_owned())
            .await?;
    }

    // Server has stopped accepting new connections. Wait for in-flight
    // executions to finish, then force-cancel any survivors.
    let grace = Duration::from_secs(config.shutdown_grace_period_secs);
    let _ = tokio::time::timeout(grace, wait_for_inflight(state.vee.clone())).await;
    state.vee.stop().await;

    tracing::info!("vico-vee shutdown complete");
    Ok(())
}

/// Wait until the executor daemon has no in-flight executions.
async fn wait_for_inflight(vee: std::sync::Arc<vico_vee::ExecutorDaemon>) {
    loop {
        let count = vee.inflight_count().await;
        if count == 0 {
            break;
        }
        tracing::info!(inflight = count, "waiting for in-flight executions");
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// Wait for SIGTERM (Unix) or SIGINT (Ctrl-C).
#[cfg(unix)]
async fn shutdown_signal() {
    let mut sigterm = match signal::unix::signal(signal::unix::SignalKind::terminate()) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!(error = %e, "failed to install SIGTERM handler; using SIGINT only");
            let _ = signal::ctrl_c().await;
            return;
        }
    };

    tokio::select! {
        _ = sigterm.recv() => tracing::info!("received SIGTERM"),
        _ = signal::ctrl_c() => tracing::info!("received SIGINT"),
    }
}

#[cfg(not(unix))]
async fn shutdown_signal() {
    let _ = signal::ctrl_c().await;
    tracing::info!("received shutdown signal");
}
