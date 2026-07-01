//! Integration tests for the standalone `vico-vee` service.

mod common;

use common::*;
use reqwest::{Client, StatusCode};
use serde_json::{json, Value};
use std::time::Duration;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use vico_vee::server::{router, AppState};

#[tokio::test]
async fn python_submit_status_list_artifacts_and_cancel() {
    let tmp = tempfile::tempdir().unwrap();
    let server = spawn_server(test_config(&tmp)).await;

    if !python_execution_available(&server).await {
        server.stop().await;
        return;
    }

    let client = Client::new();

    // Submit a simple Python task and wait for it to complete.
    let resp = submit_code(
        &client,
        &server.addr,
        ADMIN_TOKEN,
        "python",
        "print('hello from python integration test')",
        "python-agent",
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let submit: Value = resp.json().await.unwrap();
    assert!(submit["success"].as_bool().unwrap());
    let exec_id = submit["execution_id"].as_str().unwrap();

    let terminal = wait_terminal(&client, &server.addr, ADMIN_TOKEN, exec_id, None)
        .await
        .expect("python task should reach a terminal state");
    assert_eq!(terminal["data"]["status"], "Completed");

    // Status endpoint returns the same execution.
    let status = fetch_status(&client, &server.addr, ADMIN_TOKEN, exec_id, None)
        .await
        .expect("status should be present");
    assert_eq!(status["data"]["execution_id"], exec_id);

    // List endpoint contains the execution.
    let list_url = format!("http://{}/vee/list", server.addr);
    let list: Value = client
        .post(&list_url)
        .bearer_auth(ADMIN_TOKEN)
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(list["success"].as_bool().unwrap());
    let ids: Vec<&str> = list["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["execution_id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&exec_id));

    // Artifacts endpoint returns artifacts, including stdout as text.
    // The full execution result includes stdout as a text artifact.
    let stdout = terminal["data"]["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|v| v["Text"]["content"].as_str());
    assert!(stdout
        .expect("stdout artifact missing")
        .contains("hello from python integration test"));

    // The artifacts endpoint returns lightweight summaries.
    let artifacts_url = format!("http://{}/vee/artifacts", server.addr);
    let artifacts: Value = client
        .post(&artifacts_url)
        .bearer_auth(ADMIN_TOKEN)
        .json(&json!({ "execution_id": exec_id }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(artifacts["success"].as_bool().unwrap());
    let has_text_summary = artifacts["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["artifact"]["artifact_type"].as_str() == Some("text"));
    assert!(has_text_summary, "expected a text artifact summary");

    // Cancel a long-running Python task before it finishes.
    let cancel_resp = submit_code(
        &client,
        &server.addr,
        ADMIN_TOKEN,
        "python",
        "import time\ntime.sleep(30)",
        "python-agent",
        None,
    )
    .await;
    let cancel_submit: Value = cancel_resp.json().await.unwrap();
    let cancel_id = cancel_submit["execution_id"].as_str().unwrap();

    tokio::time::sleep(Duration::from_millis(100)).await;
    let cancel_url = format!("http://{}/vee/cancel", server.addr);
    let cancel: Value = client
        .post(&cancel_url)
        .bearer_auth(ADMIN_TOKEN)
        .json(&json!({ "execution_id": cancel_id }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(cancel["success"].as_bool().unwrap());

    let terminal_cancel = wait_terminal(&client, &server.addr, ADMIN_TOKEN, cancel_id, None)
        .await
        .expect("cancelled task should reach terminal state");
    assert_eq!(terminal_cancel["data"]["status"], "Cancelled");

    server.stop().await;
}

#[tokio::test]
async fn shell_submit_status_list_artifacts() {
    let tmp = tempfile::tempdir().unwrap();
    let server = spawn_server(test_config(&tmp)).await;
    let client = Client::new();

    let resp = submit_code(
        &client,
        &server.addr,
        ADMIN_TOKEN,
        "shell",
        "echo hello-shell-integration",
        "shell-agent",
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);
    let submit: Value = resp.json().await.unwrap();
    assert!(submit["success"].as_bool().unwrap());
    let exec_id = submit["execution_id"].as_str().unwrap();

    let terminal = wait_terminal(&client, &server.addr, ADMIN_TOKEN, exec_id, None)
        .await
        .expect("shell task should reach a terminal state");
    assert_eq!(
        terminal["data"]["status"],
        "Completed",
        "shell task failed: {:?}",
        terminal["data"]["error_log"].as_str()
    );

    let status = fetch_status(&client, &server.addr, ADMIN_TOKEN, exec_id, None)
        .await
        .expect("status should be present");
    assert_eq!(status["data"]["execution_id"], exec_id);

    let list_url = format!("http://{}/vee/list", server.addr);
    let list: Value = client
        .post(&list_url)
        .bearer_auth(ADMIN_TOKEN)
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let ids: Vec<&str> = list["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["execution_id"].as_str().unwrap())
        .collect();
    assert!(ids.contains(&exec_id));

    let stdout = terminal["data"]["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find_map(|v| v["Text"]["content"].as_str());
    assert!(
        stdout
            .expect(&format!(
                "stdout artifact missing: {}",
                terminal["data"]["artifacts"]
            ))
            .contains("hello-shell-integration")
    );

    let artifacts_url = format!("http://{}/vee/artifacts", server.addr);
    let artifacts: Value = client
        .post(&artifacts_url)
        .bearer_auth(ADMIN_TOKEN)
        .json(&json!({ "execution_id": exec_id }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let has_text_summary = artifacts["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .any(|v| v["artifact"]["artifact_type"].as_str() == Some("text"));
    assert!(has_text_summary, "expected a text artifact summary");

    server.stop().await;
}

#[tokio::test]
async fn auth_valid_invalid_wrong_scope_and_missing() {
    let tmp = tempfile::tempdir().unwrap();
    let server = spawn_server(test_config(&tmp)).await;
    let client = Client::new();

    // Valid admin key can submit.
    let resp = submit_code(
        &client,
        &server.addr,
        ADMIN_TOKEN,
        "shell",
        "echo ok",
        "auth-agent",
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::OK);

    // Missing Authorization header -> 401.
    let url = format!("http://{}/vee/submit", server.addr);
    let resp = client
        .post(&url)
        .header("content-type", "application/json")
        .json(&json!({
            "agent_id": "auth-agent",
            "language": "shell",
            "source_code": "echo ok",
            "capabilities": ["process_spawn"],
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // Invalid key -> 401.
    let resp = submit_code(
        &client,
        &server.addr,
        "not-a-real-token",
        "shell",
        "echo ok",
        "auth-agent",
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);

    // Read-only key submitting a task -> 403.
    let resp = submit_code(
        &client,
        &server.addr,
        READ_TOKEN,
        "shell",
        "echo ok",
        "auth-agent",
        None,
    )
    .await;
    assert_eq!(resp.status(), StatusCode::FORBIDDEN);

    // Public health endpoint does not require auth.
    let resp = client
        .get(format!("http://{}/health", server.addr))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), StatusCode::OK);

    server.stop().await;
}

#[tokio::test]
async fn rate_limit_ip_burst_returns_429_with_retry_after() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = test_config(&tmp);
    config.rate_limit.per_sec = 1;
    config.rate_limit.burst = 2;
    let server = spawn_server(config).await;
    let client = Client::new();

    let url = format!("http://{}/health", server.addr);
    let mut handles = Vec::new();
    for _ in 0..5 {
        let client = client.clone();
        let url = url.clone();
        handles.push(tokio::spawn(async move {
            client.get(&url).send().await.unwrap()
        }));
    }
    let responses: Vec<_> = futures::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();

    let ok_count = responses
        .iter()
        .filter(|r| r.status() == StatusCode::OK)
        .count();
    let rate_limited: Vec<_> = responses
        .iter()
        .filter(|r| r.status() == StatusCode::TOO_MANY_REQUESTS)
        .collect();

    assert!(ok_count >= 1, "at least one request should succeed");
    assert!(!rate_limited.is_empty(), "burst should be exceeded");
    for r in &rate_limited {
        assert!(r.headers().contains_key("retry-after"));
    }

    server.stop().await;
}

#[tokio::test]
async fn rate_limit_agent_burst_returns_429() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = test_config(&tmp);
    config.rate_limit.exec_per_sec = 1;
    config.rate_limit.exec_burst = 1;
    let server = spawn_server(config).await;
    let client = Client::new();

    let url = format!("http://{}/vee/submit", server.addr);
    let body = json!({
        "agent_id": "rate-limit-agent",
        "language": "shell",
        "source_code": "echo ok",
        "capabilities": ["process_spawn"],
    });

    let mut handles = Vec::new();
    for _ in 0..3 {
        let client = client.clone();
        let url = url.clone();
        let body = body.clone();
        handles.push(tokio::spawn(async move {
            client
                .post(&url)
                .bearer_auth(ADMIN_TOKEN)
                .json(&body)
                .send()
                .await
                .unwrap()
        }));
    }
    let responses: Vec<_> = futures::future::join_all(handles)
        .await
        .into_iter()
        .map(|r| r.unwrap())
        .collect();

    let ok_count = responses
        .iter()
        .filter(|r| r.status() == StatusCode::OK)
        .count();
    let rate_limited = responses
        .iter()
        .filter(|r| r.status() == StatusCode::TOO_MANY_REQUESTS)
        .count();

    assert!(ok_count >= 1, "at least one submission should succeed");
    assert!(rate_limited >= 1, "agent burst should be exceeded");

    server.stop().await;
}

#[tokio::test]
async fn tls_self_signed_cert_connects() {
    let tmp = tempfile::tempdir().unwrap();
    let Some((cert_path, key_path)) = generate_self_signed_cert(&tmp) else {
        return;
    };

    let mut config = test_config(&tmp);
    config.tls_cert = Some(cert_path);
    config.tls_key = Some(key_path);

    let state = AppState::try_new(config.clone())
        .await
        .expect("failed to create AppState");
    let app = router(state.clone());

    let listener = TcpListener::bind((config.bind.as_str(), 0))
        .await
        .expect("failed to bind");
    let addr = listener.local_addr().unwrap();

    let tls_config = vico_vee::tls::TlsConfig::new(
        config.tls_cert.as_ref().unwrap(),
        config.tls_key.as_ref().unwrap(),
    );
    let tls_reloader = vico_vee::tls::TlsReloader::new(&tls_config).unwrap();
    let shutdown = CancellationToken::new();
    let shutdown_clone = shutdown.clone();
    let handle = tokio::spawn(async move {
        vico_vee::tls::serve_https(
            listener,
            app,
            tls_reloader,
            shutdown_clone.cancelled_owned(),
        )
        .await
        .unwrap();
    });
    tokio::time::sleep(Duration::from_millis(50)).await;

    let client = Client::builder()
        .danger_accept_invalid_certs(true)
        .build()
        .unwrap();

    let health = client
        .get(format!("https://{}/health", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);

    let ready = client
        .get(format!("https://{}/ready", addr))
        .send()
        .await
        .unwrap();
    assert_eq!(ready.status(), StatusCode::OK);

    shutdown.cancel();
    let _ = tokio::time::timeout(Duration::from_secs(5), handle).await;
    state.vee.stop().await;
}

#[tokio::test]
async fn backup_restore_round_trip_via_http() {
    let tmp = tempfile::tempdir().unwrap();
    let server = spawn_server(test_config(&tmp)).await;
    let client = Client::new();

    // Seed the data directory with a recognizable artifact.
    let artifact_dir = server.state.config.data_dir.join("artifacts");
    std::fs::create_dir_all(&artifact_dir).unwrap();
    std::fs::write(artifact_dir.join("marker.txt"), "before-backup").unwrap();

    // Create a backup through the admin endpoint.
    let backup_url = format!("http://{}/admin/backup", server.addr);
    let backup_resp = client
        .post(&backup_url)
        .bearer_auth(ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(backup_resp.status(), StatusCode::OK);
    let backup_bytes = backup_resp.bytes().await.unwrap();
    assert!(!backup_bytes.is_empty());

    // Mutate the artifact after the backup is taken.
    std::fs::write(artifact_dir.join("marker.txt"), "after-backup").unwrap();

    // Restore from the backup tarball.
    let restore_url = format!("http://{}/admin/restore", server.addr);
    let restore_resp = client
        .post(&restore_url)
        .bearer_auth(ADMIN_TOKEN)
        .body(backup_bytes)
        .send()
        .await
        .unwrap();
    assert_eq!(restore_resp.status(), StatusCode::OK);
    let restore: Value = restore_resp.json().await.unwrap();
    assert!(restore["success"].as_bool().unwrap());

    assert_eq!(
        std::fs::read_to_string(artifact_dir.join("marker.txt")).unwrap(),
        "before-backup"
    );

    server.stop().await;
}

#[tokio::test]
async fn multi_tenancy_project_isolation() {
    let tmp = tempfile::tempdir().unwrap();
    let server = spawn_server(test_config(&tmp)).await;
    let client = Client::new();

    // Use shell tasks so the test does not depend on the Python sandbox.
    let alpha_resp = submit_code(
        &client,
        &server.addr,
        ADMIN_TOKEN,
        "shell",
        "echo alpha",
        "tenant-agent",
        Some("project-alpha"),
    )
    .await;
    let alpha_id = alpha_resp.json::<Value>().await.unwrap()["execution_id"]
        .as_str()
        .unwrap()
        .to_string();

    let beta_resp = submit_code(
        &client,
        &server.addr,
        ADMIN_TOKEN,
        "shell",
        "echo beta",
        "tenant-agent",
        Some("project-beta"),
    )
    .await;
    let beta_id = beta_resp.json::<Value>().await.unwrap()["execution_id"]
        .as_str()
        .unwrap()
        .to_string();

    wait_terminal(
        &client,
        &server.addr,
        ADMIN_TOKEN,
        &alpha_id,
        Some("project-alpha"),
    )
    .await;
    wait_terminal(
        &client,
        &server.addr,
        ADMIN_TOKEN,
        &beta_id,
        Some("project-beta"),
    )
    .await;

    // Status lookups across projects must fail (return success:false / not found).
    let cross_alpha = fetch_status(
        &client,
        &server.addr,
        ADMIN_TOKEN,
        &alpha_id,
        Some("project-beta"),
    )
    .await;
    assert!(
        cross_alpha
            .map(|v| v["success"].as_bool() == Some(false))
            .unwrap_or(true),
        "alpha execution should not be visible from project-beta"
    );

    let cross_beta = fetch_status(
        &client,
        &server.addr,
        ADMIN_TOKEN,
        &beta_id,
        Some("project-alpha"),
    )
    .await;
    assert!(
        cross_beta
            .map(|v| v["success"].as_bool() == Some(false))
            .unwrap_or(true),
        "beta execution should not be visible from project-alpha"
    );

    // List endpoints are scoped by project.
    let list_url = format!("http://{}/vee/list", server.addr);
    let list_alpha: Value = client
        .post(&list_url)
        .bearer_auth(ADMIN_TOKEN)
        .header("x-vee-project", "project-alpha")
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let alpha_ids: Vec<&str> = list_alpha["data"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v["execution_id"].as_str().unwrap())
        .collect();
    assert!(alpha_ids.contains(&alpha_id.as_str()));
    assert!(!alpha_ids.contains(&beta_id.as_str()));

    // Dashboard stats are partitioned by project.
    let dash_url = format!("http://{}/vee/dashboard", server.addr);
    let dash_alpha: Value = client
        .post(&dash_url)
        .bearer_auth(ADMIN_TOKEN)
        .header("x-vee-project", "project-alpha")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(dash_alpha["success"].as_bool().unwrap());
    assert!(dash_alpha["data"]["total"].as_i64().unwrap() >= 1);

    let dash_other: Value = client
        .post(&dash_url)
        .bearer_auth(ADMIN_TOKEN)
        .header("x-vee-project", "project-gamma")
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(dash_other["data"]["total"].as_i64().unwrap(), 0);

    server.stop().await;
}

#[tokio::test]
async fn health_ready_and_metrics_endpoints() {
    let tmp = tempfile::tempdir().unwrap();
    let server = spawn_server(test_config(&tmp)).await;
    let client = Client::new();

    let health = client
        .get(format!("http://{}/health", server.addr))
        .send()
        .await
        .unwrap();
    assert_eq!(health.status(), StatusCode::OK);
    let health_json: Value = health.json().await.unwrap();
    assert_eq!(health_json["status"], "ok");

    let ready = client
        .get(format!("http://{}/ready", server.addr))
        .send()
        .await
        .unwrap();
    assert_eq!(ready.status(), StatusCode::OK);
    let ready_json: Value = ready.json().await.unwrap();
    assert_eq!(ready_json["status"], "ready");

    let metrics = client
        .get(format!("http://{}/metrics", server.addr))
        .send()
        .await
        .unwrap();
    assert_eq!(metrics.status(), StatusCode::OK);
    let metrics_text = metrics.text().await.unwrap();
    assert!(metrics_text.contains("vee_executions_total"));

    server.stop().await;
}
