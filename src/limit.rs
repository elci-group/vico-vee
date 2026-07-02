//! Rate limiting and execution-rate guards.
//!
//! Provides three complementary limits:
//!
//! * Per-IP token-bucket limit for all requests, returning `429 Too Many
//!   Requests` with a `Retry-After` header.
//! * Per-`agent_id` execution-rate limit on task-submission routes.
//! * Per-`project_id` execution-rate limit on task-submission routes.
//!
//! Trusted proxy CIDRs can be configured so that `X-Forwarded-For` is used to
//! determine the real client IP when the direct peer is a known proxy.

use axum::{
    body::{to_bytes, Body},
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::collections::HashMap;
use std::net::{IpAddr, SocketAddr};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Token-bucket rate limiter with TTL eviction.
#[derive(Clone)]
pub struct RateLimiter {
    per_ip: Arc<Mutex<HashMap<String, Bucket>>>,
    per_agent: Arc<Mutex<HashMap<String, Bucket>>>,
    per_project: Arc<Mutex<HashMap<String, Bucket>>>,
    ip_rate: f64,
    ip_burst: f64,
    agent_rate: f64,
    agent_burst: f64,
    project_rate: f64,
    project_burst: f64,
    trusted_proxies: Vec<ipnet::IpNet>,
}

struct Bucket {
    tokens: f64,
    last_update: Instant,
    last_access: Instant,
}

impl Bucket {
    fn check(&mut self, now: Instant, rate: f64, burst: f64) -> bool {
        let elapsed = now.duration_since(self.last_update).as_secs_f64();
        self.tokens = (self.tokens + elapsed * rate).min(burst);
        self.last_update = now;
        self.last_access = now;
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

impl RateLimiter {
    /// Build a limiter from the service configuration.
    pub fn new(config: crate::config::RateLimitConfig) -> Self {
        Self {
            per_ip: Arc::new(Mutex::new(HashMap::new())),
            per_agent: Arc::new(Mutex::new(HashMap::new())),
            per_project: Arc::new(Mutex::new(HashMap::new())),
            ip_rate: config.per_sec.max(1) as f64,
            ip_burst: config.burst.max(1) as f64,
            agent_rate: config.exec_per_sec.max(1) as f64,
            agent_burst: config.exec_burst.max(1) as f64,
            project_rate: config.project_per_sec.max(1) as f64,
            project_burst: config.project_burst.max(1) as f64,
            trusted_proxies: parse_cidrs(&config.trusted_proxy_cidrs),
        }
    }

    fn check(
        store: &Mutex<HashMap<String, Bucket>>,
        key: &str,
        rate: f64,
        burst: f64,
    ) -> Result<(), Duration> {
        let mut map = store.lock().unwrap();
        let now = Instant::now();
        let bucket = map.entry(key.to_string()).or_insert_with(|| Bucket {
            tokens: burst,
            last_update: now,
            last_access: now,
        });
        if bucket.check(now, rate, burst) {
            Ok(())
        } else {
            let retry_after = Duration::from_secs_f64((1.0 / rate).ceil());
            Err(retry_after)
        }
    }

    /// Check whether a request from `ip` is allowed under the per-IP budget.
    pub fn check_ip(&self, ip: &str) -> Result<(), Duration> {
        Self::check(&self.per_ip, ip, self.ip_rate, self.ip_burst)
    }

    /// Check whether `agent_id` is allowed to submit another execution.
    pub fn check_agent(&self, agent_id: &str) -> Result<(), Duration> {
        Self::check(&self.per_agent, agent_id, self.agent_rate, self.agent_burst)
    }

    /// Check whether `project_id` is allowed to submit another execution.
    pub fn check_project(&self, project_id: &str) -> Result<(), Duration> {
        Self::check(
            &self.per_project,
            project_id,
            self.project_rate,
            self.project_burst,
        )
    }

    /// Determine the real client IP from the request, respecting trusted proxies.
    pub fn client_ip(&self, req: &Request, peer: Option<SocketAddr>) -> String {
        let peer_ip = peer.map(|s| s.ip());
        let trusted = peer_ip.map(|ip| self.is_trusted_proxy(ip)).unwrap_or(false);

        if trusted {
            if let Some(value) = req.headers().get("x-forwarded-for").and_then(|v| v.to_str().ok()) {
                // The rightmost untrusted address is the closest client.
                for part in value.split(',').rev().map(str::trim) {
                    if let Ok(ip) = part.parse::<IpAddr>() {
                        if !self.is_trusted_proxy(ip) {
                            return ip.to_string();
                        }
                    }
                }
            }
        }

        peer_ip.map(|ip| ip.to_string()).unwrap_or_else(|| "127.0.0.1".to_string())
    }

    fn is_trusted_proxy(&self, ip: IpAddr) -> bool {
        self.trusted_proxies.iter().any(|net| net.contains(&ip))
    }
}

fn parse_cidrs(cidrs: &[String]) -> Vec<ipnet::IpNet> {
    cidrs
        .iter()
        .filter_map(|s| s.parse().ok())
        .collect()
}

fn add_retry_after(mut resp: Response, retry_after: Duration) -> Response {
    if let Ok(value) = retry_after.as_secs().to_string().parse() {
        resp.headers_mut().insert("retry-after", value);
    }
    resp
}

/// Middleware that applies a per-IP token-bucket rate limit.
pub async fn ip_rate_limit_middleware(
    State(state): State<crate::server::AppState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let peer = req
        .extensions()
        .get::<ConnectInfo<SocketAddr>>()
        .map(|c| c.0);
    let ip = state.rate_limiter.client_ip(&req, peer);
    if let Err(retry_after) = state.rate_limiter.check_ip(&ip) {
        return Ok(add_retry_after(
            StatusCode::TOO_MANY_REQUESTS.into_response(),
            retry_after,
        ));
    }
    Ok(next.run(req).await)
}

/// Middleware that applies per-agent and per-project rate limits on task submission.
pub async fn agent_rate_limit_middleware(
    State(state): State<crate::server::AppState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = req.uri().path().to_owned();
    if !is_rate_limited_path(&path) {
        return Ok(next.run(req).await);
    }

    let (parts, body) = req.into_parts();
    let max_body = state
        .config
        .body_limit_mb
        .saturating_mul(1024)
        .saturating_mul(1024);
    let bytes = to_bytes(body, max_body)
        .await
        .map_err(|_| StatusCode::BAD_REQUEST)?;

    let body_json: serde_json::Value = serde_json::from_slice(&bytes).unwrap_or_default();

    let agent_id = body_json
        .get("agent_id")
        .and_then(|a| a.as_str())
        .unwrap_or("unknown")
        .to_string();

    if let Err(retry_after) = state.rate_limiter.check_agent(&agent_id) {
        return Ok(add_retry_after(
            StatusCode::TOO_MANY_REQUESTS.into_response(),
            retry_after,
        ));
    }

    let project_id = parts
        .headers
        .get("x-vee-project")
        .and_then(|v| v.to_str().ok())
        .map(String::from)
        .or_else(|| body_json.get("project_id").and_then(|v| v.as_str()).map(String::from))
        .unwrap_or_else(|| crate::tenant::DEFAULT_PROJECT.to_string());

    if let Err(retry_after) = state.rate_limiter.check_project(&project_id) {
        return Ok(add_retry_after(
            StatusCode::TOO_MANY_REQUESTS.into_response(),
            retry_after,
        ));
    }

    let new_req = Request::from_parts(parts, Body::from(bytes));
    Ok(next.run(new_req).await)
}

fn is_rate_limited_path(path: &str) -> bool {
    matches!(
        path,
        "/vee/submit" | "/vee/diff" | "/vee/merge" | "/vee/reject"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::RateLimitConfig;

    fn test_config() -> RateLimitConfig {
        RateLimitConfig {
            per_sec: 1,
            burst: 1,
            exec_per_sec: 1,
            exec_burst: 1,
            project_per_sec: 1,
            project_burst: 1,
            trusted_proxy_cidrs: vec![],
        }
    }

    #[test]
    fn token_bucket_allows_burst_then_limits() {
        let limiter = RateLimiter::new(test_config());
        assert!(limiter.check_ip("1.2.3.4").is_ok());
        assert!(limiter.check_ip("1.2.3.4").is_err());
        assert!(limiter.check_ip("5.6.7.8").is_ok());
    }

    #[test]
    fn agent_rate_limits_are_separate() {
        let limiter = RateLimiter::new(test_config());
        assert!(limiter.check_agent("agent-a").is_ok());
        assert!(limiter.check_agent("agent-a").is_err());
        assert!(limiter.check_agent("agent-b").is_ok());
    }

    #[test]
    fn project_rate_limits_are_separate() {
        let limiter = RateLimiter::new(test_config());
        assert!(limiter.check_project("project-a").is_ok());
        assert!(limiter.check_project("project-a").is_err());
        assert!(limiter.check_project("project-b").is_ok());
    }

    #[test]
    fn retry_after_is_non_zero() {
        let limiter = RateLimiter::new(test_config());
        assert!(limiter.check_ip("1.2.3.4").is_ok());
        let err = limiter.check_ip("1.2.3.4").unwrap_err();
        assert!(err.as_secs() >= 1);
    }

    #[test]
    fn client_ip_uses_x_forwarded_for_from_trusted_proxy() {
        let mut config = test_config();
        config.trusted_proxy_cidrs = vec!["127.0.0.0/8".to_string()];
        let limiter = RateLimiter::new(config);

        let req = axum::http::Request::builder()
            .header("x-forwarded-for", "203.0.113.5, 127.0.0.1")
            .body(axum::body::Body::empty())
            .unwrap();
        let peer = SocketAddr::from(([127, 0, 0, 1], 12345));
        assert_eq!(limiter.client_ip(&req, Some(peer)), "203.0.113.5");
    }

    #[test]
    fn client_ip_ignores_x_forwarded_for_from_untrusted_peer() {
        let limiter = RateLimiter::new(test_config());

        let req = axum::http::Request::builder()
            .header("x-forwarded-for", "203.0.113.5")
            .body(axum::body::Body::empty())
            .unwrap();
        let peer = SocketAddr::from(([198, 51, 100, 1], 12345));
        assert_eq!(limiter.client_ip(&req, Some(peer)), "198.51.100.1");
    }
}
