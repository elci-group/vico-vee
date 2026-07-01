//! Rate limiting and execution-rate guards.
//!
//! Provides two complementary limits:
//!
//! * Per-IP token-bucket limit for all requests, returning `429 Too Many
//!   Requests` with a `Retry-After` header.
//! * Per-`agent_id` execution-rate limit on task-submission routes, preventing
//!   a single agent from flooding the queue.

use axum::{
    body::{to_bytes, Body},
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
};
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};

/// Token-bucket rate limiter.
pub struct RateLimiter {
    per_ip: Mutex<HashMap<String, Bucket>>,
    per_agent: Mutex<HashMap<String, Bucket>>,
    ip_rate: f64,
    ip_burst: f64,
    agent_rate: f64,
    agent_burst: f64,
}

struct Bucket {
    tokens: f64,
    last_update: Instant,
}

impl Bucket {
    fn check(&mut self, now: Instant, rate: f64, burst: f64) -> bool {
        let elapsed = now.duration_since(self.last_update).as_secs_f64();
        self.tokens = (self.tokens + elapsed * rate).min(burst);
        self.last_update = now;
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
            per_ip: Mutex::new(HashMap::new()),
            per_agent: Mutex::new(HashMap::new()),
            ip_rate: config.per_sec.max(1) as f64,
            ip_burst: config.burst.max(1) as f64,
            agent_rate: config.exec_per_sec.max(1) as f64,
            agent_burst: config.exec_burst.max(1) as f64,
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
}

fn add_retry_after(mut resp: Response, retry_after: Duration) -> Response {
    if let Ok(value) = retry_after.as_secs().to_string().parse() {
        resp.headers_mut().insert("retry-after", value);
    }
    resp
}

/// Middleware that applies a per-IP token-bucket rate limit.
pub async fn ip_rate_limit_middleware(
    connect_info: Option<ConnectInfo<SocketAddr>>,
    State(state): State<crate::server::AppState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let ip = connect_info
        .map(|c| c.0.ip().to_string())
        .unwrap_or_else(|| "127.0.0.1".to_string());
    if let Err(retry_after) = state.rate_limiter.check_ip(&ip) {
        return Ok(add_retry_after(
            StatusCode::TOO_MANY_REQUESTS.into_response(),
            retry_after,
        ));
    }
    Ok(next.run(req).await)
}

/// Middleware that applies a per-agent execution rate limit on task submission.
pub async fn agent_rate_limit_middleware(
    State(state): State<crate::server::AppState>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let path = req.uri().path().to_owned();
    if !is_agent_rate_limited_path(&path) {
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

    let agent_id: Option<String> = serde_json::from_slice::<serde_json::Value>(&bytes)
        .ok()
        .and_then(|v| v.get("agent_id").and_then(|a| a.as_str()).map(String::from));

    let agent_id = agent_id.unwrap_or_else(|| "unknown".to_string());

    if let Err(retry_after) = state.rate_limiter.check_agent(&agent_id) {
        return Ok(add_retry_after(
            StatusCode::TOO_MANY_REQUESTS.into_response(),
            retry_after,
        ));
    }

    let new_req = Request::from_parts(parts, Body::from(bytes));
    Ok(next.run(new_req).await)
}

fn is_agent_rate_limited_path(path: &str) -> bool {
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
    fn retry_after_is_non_zero() {
        let limiter = RateLimiter::new(test_config());
        assert!(limiter.check_ip("1.2.3.4").is_ok());
        let err = limiter.check_ip("1.2.3.4").unwrap_err();
        assert!(err.as_secs() >= 1);
    }
}
