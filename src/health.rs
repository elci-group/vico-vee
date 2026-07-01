//! Health probes, Prometheus metrics, and request-ID middleware for `vico-vee`.
//!
//! Exposes:
//! - `GET /health` — lightweight liveness probe.
//! - `GET /ready` — readiness probe that checks the executor daemon is running.
//! - `GET /metrics` — Prometheus text-format metrics.
//!
//! Also provides a thin middleware layer that ensures every request has a
//! unique `x-request-id` propagated to the response.

use axum::{
    body::Body,
    extract::{Request, State},
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::collections::HashMap;
use std::fmt::Write;
use std::sync::{Arc, Mutex};

/// Shared request context used by handlers that need the request ID.
#[derive(Clone, Debug, Default)]
pub struct RequestContext {
    pub request_id: String,
}

/// A minimal Prometheus-style metrics registry.
///
/// Stores counters and gauges and renders them in Prometheus text format.
/// Thread-safe via an internal `Mutex` so handlers and background tasks can
/// update it concurrently.
#[derive(Clone, Default)]
pub struct MetricsRegistry {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    counters: HashMap<String, u64>,
    gauges: HashMap<String, f64>,
}

impl MetricsRegistry {
    /// Increment a counter by `value`.
    pub fn counter_inc(&self, name: &str, value: u64) {
        let mut inner = self.inner.lock().unwrap();
        *inner.counters.entry(name.to_string()).or_insert(0) += value;
    }

    /// Set a gauge to `value`.
    pub fn gauge_set(&self, name: &str, value: f64) {
        let mut inner = self.inner.lock().unwrap();
        inner.gauges.insert(name.to_string(), value);
    }

    /// Render all metrics in Prometheus exposition format.
    pub fn render(&self) -> String {
        let inner = self.inner.lock().unwrap();
        let mut out = String::new();

        for (name, value) in &inner.counters {
            writeln!(out, "# TYPE {name} counter").unwrap();
            writeln!(out, "{name} {value}").unwrap();
        }

        for (name, value) in &inner.gauges {
            writeln!(out, "# TYPE {name} gauge").unwrap();
            writeln!(out, "{name} {value}").unwrap();
        }

        out
    }

    /// Update gauges derived from the current execution store.
    pub fn refresh_daemon_gauges(&self, state: &crate::server::AppState) {
        // Spawn a short-lived async block because dashboard_stats is async.
        let registry = self.clone();
        let state = state.clone();
        tokio::spawn(async move {
            let stats = state.vee.dashboard_stats(Some(crate::tenant::DEFAULT_PROJECT)).await;
            if let Some(total) = stats.get("total").and_then(|v| v.as_i64()) {
                registry.gauge_set("vee_executions_total", total as f64);
            }
            if let Some(pending) = stats.get("pending").and_then(|v| v.as_i64()) {
                registry.gauge_set("vee_executions_pending", pending as f64);
            }
            if let Some(completed) = stats.get("completed").and_then(|v| v.as_i64()) {
                registry.gauge_set("vee_executions_completed", completed as f64);
            }
            if let Some(failed) = stats.get("failed").and_then(|v| v.as_i64()) {
                registry.gauge_set("vee_executions_failed", failed as f64);
            }
        });
    }
}

/// `GET /health` — always returns a lightweight success response.
pub async fn health() -> impl IntoResponse {
    (
        StatusCode::OK,
        axum::Json(serde_json::json!({
            "status": "ok",
            "service": "vico-vee",
        })),
    )
}

/// `GET /ready` — returns 200 only when the executor daemon is running.
pub async fn ready(State(state): State<crate::server::AppState>) -> impl IntoResponse {
    // The daemon is considered ready if it was started and has not been
    // explicitly stopped. We approximate this by checking whether the
    // background handle is present.
    let handle_set = state.vee.handle_set().await;
    if handle_set {
        (
            StatusCode::OK,
            axum::Json(serde_json::json!({
                "status": "ready",
                "service": "vico-vee",
            })),
        )
            .into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            axum::Json(serde_json::json!({
                "status": "not ready",
                "service": "vico-vee",
            })),
        )
            .into_response()
    }
}

/// `GET /metrics` — Prometheus text-format metrics.
pub async fn metrics(State(state): State<crate::server::AppState>) -> impl IntoResponse {
    state.metrics.refresh_daemon_gauges(&state);
    let body = state.metrics.render();

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        body,
    )
}

/// Middleware that ensures every request carries a request ID.
///
/// If the incoming request already has an `x-request-id` header it is preserved
/// and echoed back on the response. Otherwise a fresh UUID is generated.
pub async fn set_request_id(mut request: Request<Body>, next: Next) -> Response {
    let request_id = extract_or_generate_id(request.headers());
    let header_value = match header::HeaderValue::from_str(&request_id) {
        Ok(v) => v,
        Err(_) => header::HeaderValue::from_static("invalid-request-id"),
    };

    request.headers_mut().insert("x-request-id", header_value.clone());

    let mut response = next.run(request).await;
    response.headers_mut().insert("x-request-id", header_value);
    response
}

fn extract_or_generate_id(headers: &HeaderMap) -> String {
    headers
        .get("x-request-id")
        .and_then(|v| v.to_str().ok())
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metrics_counter_increments() {
        let m = MetricsRegistry::default();
        m.counter_inc("foo", 1);
        m.counter_inc("foo", 2);
        let rendered = m.render();
        assert!(rendered.contains("# TYPE foo counter"));
        assert!(rendered.contains("foo 3"));
    }

    #[test]
    fn metrics_gauge_overwrites() {
        let m = MetricsRegistry::default();
        m.gauge_set("bar", 1.5);
        m.gauge_set("bar", 2.5);
        let rendered = m.render();
        assert!(rendered.contains("# TYPE bar gauge"));
        assert!(rendered.contains("bar 2.5"));
    }

    #[test]
    fn extract_or_generate_id_preserves_existing() {
        let mut headers = HeaderMap::new();
        headers.insert("x-request-id", header::HeaderValue::from_static("abc"));
        assert_eq!(extract_or_generate_id(&headers), "abc");
    }

    #[test]
    fn extract_or_generate_id_generates_missing() {
        let headers = HeaderMap::new();
        let id = extract_or_generate_id(&headers);
        assert!(!id.is_empty());
    }
}
