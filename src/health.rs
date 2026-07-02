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
use std::collections::{BTreeMap, HashMap};
use std::fmt::Write;
use std::sync::{Arc, Mutex};

/// Shared request context used by handlers that need the request ID.
#[derive(Clone, Debug, Default)]
pub struct RequestContext {
    pub request_id: String,
}

type Labels = BTreeMap<String, String>;

/// A minimal Prometheus-style metrics registry.
///
/// Stores counters, gauges, and histograms with label sets and renders them in
/// Prometheus text format. Thread-safe via an internal `Mutex` so handlers and
/// background tasks can update it concurrently.
#[derive(Clone, Default)]
pub struct MetricsRegistry {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Default)]
struct Inner {
    counters: HashMap<String, HashMap<Labels, u64>>,
    gauges: HashMap<String, HashMap<Labels, f64>>,
    histograms: HashMap<String, HashMap<Labels, Histogram>>,
}

#[derive(Clone)]
struct Histogram {
    buckets: Vec<f64>,
    counts: Vec<u64>,
    sum: f64,
    count: u64,
}

impl Default for Histogram {
    fn default() -> Self {
        // Standard Prometheus latency buckets (seconds).
        let buckets = vec![
            0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
        ];
        Self {
            counts: vec![0; buckets.len()],
            buckets,
            sum: 0.0,
            count: 0,
        }
    }
}

impl Histogram {
    fn observe(&mut self, value: f64) {
        self.sum += value;
        self.count += 1;
        for (i, bucket) in self.buckets.iter().enumerate() {
            if value <= *bucket {
                self.counts[i] += 1;
            }
        }
    }
}

impl MetricsRegistry {
    /// Increment a counter by `value` with an empty label set.
    pub fn counter_inc(&self, name: &str, value: u64) {
        self.counter_inc_with_labels(name, &[], value);
    }

    /// Increment a counter by `value` with the supplied labels.
    pub fn counter_inc_with_labels(&self, name: &str, labels: &[(&str, &str)], value: u64) {
        let mut inner = self.inner.lock().unwrap();
        let labels = labels_to_map(labels);
        *inner
            .counters
            .entry(name.to_string())
            .or_default()
            .entry(labels)
            .or_insert(0) += value;
    }

    /// Set a gauge to `value` with an empty label set.
    pub fn gauge_set(&self, name: &str, value: f64) {
        self.gauge_set_with_labels(name, &[], value);
    }

    /// Set a gauge to `value` with the supplied labels.
    pub fn gauge_set_with_labels(&self, name: &str, labels: &[(&str, &str)], value: f64) {
        let mut inner = self.inner.lock().unwrap();
        let labels = labels_to_map(labels);
        inner
            .gauges
            .entry(name.to_string())
            .or_default()
            .insert(labels, value);
    }

    /// Observe a value for a histogram metric.
    pub fn histogram_observe(&self, name: &str, labels: &[(&str, &str)], value: f64) {
        let mut inner = self.inner.lock().unwrap();
        let labels = labels_to_map(labels);
        inner
            .histograms
            .entry(name.to_string())
            .or_default()
            .entry(labels)
            .or_default()
            .observe(value);
    }

    /// Render all metrics in Prometheus exposition format.
    pub fn render(&self) -> String {
        let inner = self.inner.lock().unwrap();
        let mut out = String::new();

        for (name, series) in &inner.counters {
            writeln!(out, "# TYPE {name} counter").unwrap();
            for (labels, value) in series {
                let label_str = format_labels(labels);
                writeln!(out, "{name}{label_str} {value}").unwrap();
            }
        }

        for (name, series) in &inner.gauges {
            writeln!(out, "# TYPE {name} gauge").unwrap();
            for (labels, value) in series {
                let label_str = format_labels(labels);
                writeln!(out, "{name}{label_str} {value}").unwrap();
            }
        }

        for (name, series) in &inner.histograms {
            writeln!(out, "# TYPE {name} histogram").unwrap();
            for (labels, hist) in series {
                let label_str = format_labels(labels);
                for (bucket, count) in hist.buckets.iter().zip(hist.counts.iter()) {
                    let bucket_label = format_labels_with_extra(labels, "le", &bucket.to_string());
                    writeln!(out, "{name}_bucket{bucket_label} {count}",).unwrap();
                }
                writeln!(out, "{name}_sum{label_str} {}", hist.sum).unwrap();
                writeln!(out, "{name}_count{label_str} {}", hist.count).unwrap();
            }
        }

        out
    }

    /// Update gauges derived from the current execution store.
    pub async fn refresh_daemon_gauges(&self, state: &crate::server::AppState) {
        let stats = state.vee.dashboard_stats(None).await;
        if let Some(total) = stats.get("total").and_then(|v| v.as_i64()) {
            self.gauge_set("vee_executions_total", total as f64);
        }
        if let Some(pending) = stats.get("pending").and_then(|v| v.as_i64()) {
            self.gauge_set("vee_executions_pending", pending as f64);
        }
        if let Some(completed) = stats.get("completed").and_then(|v| v.as_i64()) {
            self.gauge_set("vee_executions_completed", completed as f64);
        }
        if let Some(failed) = stats.get("failed").and_then(|v| v.as_i64()) {
            self.gauge_set("vee_executions_failed", failed as f64);
        }
    }
}

fn labels_to_map(labels: &[(&str, &str)]) -> Labels {
    labels
        .iter()
        .map(|(k, v)| (k.to_string(), v.to_string()))
        .collect()
}

fn format_labels(labels: &Labels) -> String {
    if labels.is_empty() {
        String::new()
    } else {
        let parts: Vec<String> = labels
            .iter()
            .map(|(k, v)| format!("{k}=\"{}\"", escape_label_value(v)))
            .collect();
        format!("{{{}}}", parts.join(","))
    }
}

fn format_labels_with_extra(labels: &Labels, key: &str, value: &str) -> String {
    let mut extended = labels.clone();
    extended.insert(key.to_string(), value.to_string());
    format_labels(&extended)
}

fn escape_label_value(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
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

/// `GET /ready` — returns 200 only when the executor daemon is running and
/// the persistent store is reachable.
pub async fn ready(State(state): State<crate::server::AppState>) -> impl IntoResponse {
    let running = state.vee.is_running().await;
    let db_healthy = state.vee.db_healthy().await;
    if running && db_healthy {
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
    state.metrics.refresh_daemon_gauges(&state).await;
    let body = state.metrics.render();

    (
        StatusCode::OK,
        [(header::CONTENT_TYPE, "text/plain; charset=utf-8")],
        body,
    )
}

/// Middleware that records per-request Prometheus metrics.
///
/// Emits:
/// - `vee_requests_total{method, route, status}` counter
/// - `vee_request_duration_seconds{method, route}` histogram
pub async fn request_metrics_middleware(
    State(state): State<crate::server::AppState>,
    request: Request<Body>,
    next: Next,
) -> Response {
    let method = request.method().to_string();
    let route = request.uri().path().to_string();
    let start = std::time::Instant::now();

    let response = next.run(request).await;
    let status = response.status().as_u16().to_string();
    let duration_secs = start.elapsed().as_secs_f64();

    state.metrics.counter_inc_with_labels(
        "vee_requests_total",
        &[("method", &method), ("route", &route), ("status", &status)],
        1,
    );
    state.metrics.histogram_observe(
        "vee_request_duration_seconds",
        &[("method", &method), ("route", &route)],
        duration_secs,
    );

    response
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

    request
        .headers_mut()
        .insert("x-request-id", header_value.clone());

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
    fn metrics_histogram_observes_values() {
        let m = MetricsRegistry::default();
        m.histogram_observe("baz", &[("route", "/health")], 0.01);
        m.histogram_observe("baz", &[("route", "/health")], 0.05);
        let rendered = m.render();
        assert!(rendered.contains("# TYPE baz histogram"));
        assert!(rendered.contains("baz_count{route=\"/health\"} 2"));
        assert!(rendered.contains("baz_sum{route=\"/health\"} 0.06"));
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
