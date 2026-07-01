//! Shared helpers for `vico-vee` integration and load tests.

use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use vico_vee::config::{ApiKeysConfig, Config, RateLimitConfig};
use vico_vee::server::{router, AppState};

pub const ADMIN_TOKEN: &str = "test-admin-token";
pub const SUBMIT_TOKEN: &str = "test-submit-token";
pub const READ_TOKEN: &str = "test-read-token";

const API_KEYS_TOML: &str = r#"
[keys.admin]
token = "test-admin-token"
scopes = ["submit", "read", "admin"]

[keys.submit]
token = "test-submit-token"
scopes = ["submit"]

[keys.read]
token = "test-read-token"
scopes = ["read"]
"#;

/// A running test server together with its state and scratch directory.
pub struct TestServer {
    pub addr: SocketAddr,
    pub state: AppState,
    pub shutdown: CancellationToken,
    pub handle: tokio::task::JoinHandle<()>,
    pub tmp: TempDir,
}

impl TestServer {
    /// Build an HTTP URL for `path`.
    pub fn url(&self, path: &str) -> String {
        format!("http://{}{}", self.addr, path)
    }

    /// Gracefully stop the server and the executor daemon.
    pub async fn stop(self) {
        self.shutdown.cancel();
        let _ = tokio::time::timeout(Duration::from_secs(10), self.handle).await;
        self.state.vee.stop().await;
        // `tmp` is dropped after this point.
    }
}

/// Create a scratch directory and write the test API-keys file.
pub fn test_config(tmp: &TempDir) -> Config {
    let data_dir = tmp.path().join("data");
    let config_dir = tmp.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    let keys_path = config_dir.join("api_keys.toml");
    std::fs::write(&keys_path, API_KEYS_TOML).unwrap();

    Config {
        bind: "127.0.0.1".to_string(),
        port: 0,
        data_dir,
        config_dir: config_dir.clone(),
        api_keys: ApiKeysConfig {
            file: keys_path,
            require_auth: true,
            env_override: None,
        },
        body_limit_mb: 16,
        request_timeout_secs: 30,
        rate_limit: RateLimitConfig {
            per_sec: 1000,
            burst: 1000,
            exec_per_sec: 1000,
            exec_burst: 1000,
        },
        shutdown_grace_period_secs: 30,
        ..Config::default()
    }
}

/// Spawn the full HTTP stack on a random port and return the test server.
pub async fn spawn_server(config: Config) -> TestServer {
    let state = AppState::try_new(config.clone())
        .await
        .expect("failed to create AppState");
    let app = router(state.clone());

    let listener = TcpListener::bind((config.bind.as_str(), 0))
        .await
        .expect("failed to bind");
    let addr = listener.local_addr().unwrap();

    let shutdown = CancellationToken::new();
    let shutdown_clone = shutdown.clone();
    let handle = tokio::spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_clone.cancelled_owned())
            .await
            .expect("server failed");
    });

    // Give the server a moment to start accepting connections.
    tokio::time::sleep(Duration::from_millis(50)).await;

    TestServer {
        addr,
        state,
        shutdown,
        handle,
        tmp: TempDir::new().expect("tempdir"),
    }
}

/// Submit a task for execution.
pub async fn submit_code(
    client: &Client,
    addr: &SocketAddr,
    token: &str,
    language: &str,
    code: &str,
    agent_id: &str,
    project: Option<&str>,
) -> reqwest::Response {
    let url = format!("http://{}/vee/submit", addr);
    let body = json!({
        "agent_id": agent_id,
        "language": language,
        "source_code": code,
        "capabilities": ["process_spawn"],
    });
    let mut req = client
        .post(&url)
        .bearer_auth(token)
        .header("content-type", "application/json")
        .json(&body);
    if let Some(p) = project {
        req = req.header("x-vee-project", p);
    }
    req.send().await.unwrap()
}

/// Fetch the status for an execution id.
pub async fn fetch_status(
    client: &Client,
    addr: &SocketAddr,
    token: &str,
    execution_id: &str,
    project: Option<&str>,
) -> Option<Value> {
    let url = format!("http://{}/vee/status", addr);
    let body = json!({ "execution_id": execution_id });
    let mut req = client.post(&url).bearer_auth(token).json(&body);
    if let Some(p) = project {
        req = req.header("x-vee-project", p);
    }
    let resp = req.send().await.ok()?;
    if resp.status() != StatusCode::OK {
        return None;
    }
    resp.json::<Value>().await.ok()
}

/// Poll until the execution reaches a terminal status.
pub async fn wait_terminal(
    client: &Client,
    addr: &SocketAddr,
    token: &str,
    execution_id: &str,
    project: Option<&str>,
) -> Option<Value> {
    for _ in 0..120 {
        if let Some(value) = fetch_status(client, addr, token, execution_id, project).await {
            if let Some(status) = value["data"]["status"].as_str() {
                if matches!(status, "completed" | "failed" | "cancelled") {
                    return Some(value);
                }
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    None
}

/// Return `true` if a no-op Python task reaches `completed`.
///
/// Tests that require the Python sandbox should return early when this is `false`.
pub async fn python_execution_available(server: &TestServer) -> bool {
    let client = Client::new();
    let resp = submit_code(
        &client,
        &server.addr,
        ADMIN_TOKEN,
        "python",
        "print('probe')",
        "probe-agent",
        None,
    )
    .await;
    if !resp.status().is_success() {
        return false;
    }
    let value: Value = match resp.json().await {
        Ok(v) => v,
        Err(_) => return false,
    };
    let Some(id) = value["execution_id"].as_str() else {
        return false;
    };
    let terminal = wait_terminal(&client, &server.addr, ADMIN_TOKEN, id, None).await;
    terminal
        .and_then(|v| v["data"]["status"].as_str().map(String::from))
        == Some("completed".to_string())
}

/// Generate a self-signed certificate/key pair using `openssl`.
pub fn generate_self_signed_cert(tmp: &TempDir) -> Option<(PathBuf, PathBuf)> {
    let key_path = tmp.path().join("key.pem");
    let cert_path = tmp.path().join("cert.pem");
    let status = std::process::Command::new("openssl")
        .args([
            "req",
            "-x509",
            "-newkey",
            "rsa:2048",
            "-keyout",
            key_path.to_str().unwrap(),
            "-out",
            cert_path.to_str().unwrap(),
            "-days",
            "1",
            "-nodes",
            "-subj",
            "/CN=localhost",
            "-addext",
            "subjectAltName=DNS:localhost,IP:127.0.0.1",
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    if status.map(|s| s.success()).unwrap_or(false) {
        Some((cert_path, key_path))
    } else {
        None
    }
}
