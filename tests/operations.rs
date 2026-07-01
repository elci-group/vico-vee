//! Integration tests for TLS, operations, and observability features.

use axum::body::Body;
use axum::http::{Request, StatusCode};
use std::time::Duration;
use tower::ServiceExt;
use vico_vee::config::Config;
use vico_vee::server::{router, AppState};

fn test_config() -> Config {
    Config {
        bind: "127.0.0.1".to_string(),
        port: 0,
        ..Config::default()
    }
}

#[tokio::test]
async fn health_endpoint_returns_ok() {
    let state = AppState::test_new(test_config());
    let app = router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
}

#[tokio::test]
async fn ready_endpoint_returns_ok_when_daemon_started() {
    let state = AppState::test_new(test_config());
    state.vee.start().await;
    let app = router(state.clone());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/ready")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    state.vee.stop().await;
}

#[tokio::test]
async fn metrics_endpoint_returns_prometheus_text() {
    let state = AppState::test_new(test_config());
    state.vee.start().await;
    let app = router(state.clone());

    let response = app
        .oneshot(
            Request::builder()
                .uri("/metrics")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get("content-type")
        .and_then(|v| v.to_str().ok());
    assert_eq!(content_type, Some("text/plain; charset=utf-8"));
    state.vee.stop().await;
}

#[tokio::test]
async fn request_id_is_propagated() {
    let state = AppState::test_new(test_config());
    let app = router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .header("x-request-id", "test-req-123")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    assert_eq!(
        response
            .headers()
            .get("x-request-id")
            .and_then(|v| v.to_str().ok()),
        Some("test-req-123")
    );
}

#[tokio::test]
async fn request_id_is_generated_when_missing() {
    let state = AppState::test_new(test_config());
    let app = router(state);

    let response = app
        .oneshot(
            Request::builder()
                .uri("/health")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let id = response
        .headers()
        .get("x-request-id")
        .and_then(|v| v.to_str().ok());
    assert!(id.is_some());
    assert!(!id.unwrap().is_empty());
}

#[tokio::test]
async fn graceful_shutdown_stops_server() {
    let config = test_config();
    let state = AppState::test_new(config.clone());
    state.vee.start().await;
    let app = router(state.clone());

    let listener = tokio::net::TcpListener::bind((config.bind.as_str(), 0))
        .await
        .unwrap();
    let addr = listener.local_addr().unwrap();

    let shutdown = tokio_util::sync::CancellationToken::new();
    let shutdown_clone = shutdown.clone();

    let server_handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_clone.cancelled_owned())
            .await
            .unwrap();
    });

    // Give the server a moment to start.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // Verify the server is reachable.
    let client = reqwest::Client::new();
    let resp = client
        .get(format!("http://{addr}/health"))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);

    // Trigger graceful shutdown.
    shutdown.cancel();

    // The server task should complete.
    tokio::time::timeout(Duration::from_secs(5), server_handle)
        .await
        .expect("server should shut down before timeout")
        .expect("server task should complete");

    state.vee.stop().await;
}
