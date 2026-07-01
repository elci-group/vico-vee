//! Load and chaos tests for the standalone `vico-vee` service.

mod common;

use common::*;
use flate2::write::GzEncoder;
use flate2::Compression;
use reqwest::{Client, StatusCode};
use std::io::Read;
use std::time::Duration;
use tar::Builder;
use vico_vee::config::RateLimitConfig;

#[tokio::test]
async fn concurrent_cpu_and_sleep_tasks_reach_terminal() {
    let tmp = tempfile::tempdir().unwrap();
    let server = spawn_server(test_config(&tmp)).await;
    let client = Client::new();

    let use_python = python_execution_available(&server).await;
    let concurrency = 20;
    let mut ids = Vec::new();

    let mut handles = Vec::new();
    for i in 0..concurrency {
        let client = client.clone();
        let addr = server.addr;
        let is_cpu = i % 2 == 0;
        let (language, code) = if use_python && is_cpu {
            ("python", "sum(range(100000))")
        } else {
            ("shell", "sleep 1")
        };
        handles.push(tokio::spawn(async move {
            let resp = submit_code(
                &client,
                &addr,
                ADMIN_TOKEN,
                language,
                code,
                &format!("load-agent-{i}"),
                None,
            )
            .await;
            let json: serde_json::Value = resp.json().await.unwrap();
            json["execution_id"].as_str().unwrap().to_string()
        }));
    }
    for handle in handles {
        ids.push(handle.await.unwrap());
    }

    for id in &ids {
        let terminal = wait_terminal(&client, &server.addr, ADMIN_TOKEN, id, None)
            .await
            .expect("every task should reach a terminal state");
        let status = terminal["data"]["status"].as_str().unwrap();
        assert!(
            matches!(status, "completed" | "failed" | "cancelled"),
            "unexpected terminal status: {status}"
        );
    }

    server.stop().await;
}

#[tokio::test]
async fn graceful_shutdown_under_load_preserves_terminal_statuses() {
    let tmp = tempfile::tempdir().unwrap();
    let server = spawn_server(test_config(&tmp)).await;
    let client = Client::new();
    let addr = server.addr;
    let shutdown = server.shutdown.clone();
    let handle = server.handle;
    let state = server.state;

    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    // Fire submissions continuously for a short window.
    let submitter = tokio::spawn(async move {
        let mut counter = 0usize;
        loop {
            let client = client.clone();
            let tx = tx.clone();
            let id = counter;
            tokio::spawn(async move {
                let resp = submit_code(
                    &client,
                    &addr,
                    ADMIN_TOKEN,
                    "shell",
                    "sleep 0.5",
                    &format!("chaos-agent-{id}"),
                    None,
                )
                .await;
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    if let Some(exec_id) = json["execution_id"].as_str() {
                        let _ = tx.send(exec_id.to_string());
                    }
                }
            });
            counter += 1;
            if counter >= 200 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        drop(tx);
    });

    // Let load build up, then trigger graceful shutdown.
    tokio::time::sleep(Duration::from_millis(400)).await;
    shutdown.cancel();

    let _server_result = tokio::time::timeout(Duration::from_secs(15), handle).await;
    submitter.await.unwrap();

    // Collect all ids that were successfully submitted.
    let mut ids = Vec::new();
    while let Some(id) = rx.recv().await {
        ids.push(id);
    }

    // Wait for any in-flight executions to finish now that the listener is closed.
    for _ in 0..60 {
        if state.vee.inflight_count().await == 0 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let mut lost = 0usize;
    for id in &ids {
        if let Some(result) = state.vee.get_status(id, Some("default")).await {
            let status = format!("{:?}", result.status);
            if !matches!(
                result.status,
                vico_vee::types::ExecutionStatus::Completed
                    | vico_vee::types::ExecutionStatus::Failed
                    | vico_vee::types::ExecutionStatus::Cancelled
            ) {
                lost += 1;
                eprintln!("non-terminal status for {id}: {status}");
            }
        } else {
            lost += 1;
            eprintln!("missing status for {id}");
        }
    }

    assert!(
        lost == 0,
        "{lost} / {} submitted executions lost their terminal status",
        ids.len()
    );

    state.vee.stop().await;
    // Avoid dropping the partially-moved TestServer; `tmp` is dropped here.
    drop(tmp);
}

#[tokio::test]
async fn large_artifact_upload_download_round_trip() {
    let tmp = tempfile::tempdir().unwrap();
    let mut config = test_config(&tmp);
    config.body_limit_mb = 2;
    config.rate_limit = RateLimitConfig {
        per_sec: 1000,
        burst: 1000,
        exec_per_sec: 1000,
        exec_burst: 1000,
    };
    let server = spawn_server(config).await;
    let client = Client::new();

    // Build a ~1.5 MiB tarball containing a single large file.
    let payload_size = 1_500_000usize;
    let data: Vec<u8> = (0..payload_size).map(|i| (i % 256) as u8).collect();

    let mut tarball = Vec::new();
    {
        let enc = GzEncoder::new(&mut tarball, Compression::default());
        let mut builder = Builder::new(enc);
        let mut header = tar::Header::new_gnu();
        header.set_path("artifacts/large.bin").unwrap();
        header.set_size(data.len() as u64);
        header.set_mode(0o644);
        header.set_cksum();
        builder.append(&header, data.as_slice()).unwrap();
        builder.finish().unwrap();
    }
    assert!(tarball.len() > 1_000_000 && tarball.len() < 2_000_000);

    // Upload (restore) the tarball.
    let restore_url = format!("http://{}/admin/restore", server.addr);
    let restore_resp = client
        .post(&restore_url)
        .bearer_auth(ADMIN_TOKEN)
        .body(tarball.clone())
        .send()
        .await
        .unwrap();
    assert_eq!(restore_resp.status(), StatusCode::OK);

    // Verify the restored file on disk.
    let restored_path = server
        .state
        .config
        .data_dir
        .join("artifacts")
        .join("large.bin");
    let restored = tokio::fs::read(&restored_path).await.unwrap();
    assert_eq!(restored.len(), payload_size);
    assert_eq!(restored, data);

    // Download (backup) the data directory and verify the file round-trips.
    let backup_url = format!("http://{}/admin/backup", server.addr);
    let backup_resp = client
        .post(&backup_url)
        .bearer_auth(ADMIN_TOKEN)
        .send()
        .await
        .unwrap();
    assert_eq!(backup_resp.status(), StatusCode::OK);
    let backup_bytes = backup_resp.bytes().await.unwrap();

    let mut found = false;
    let dec = flate2::read::GzDecoder::new(backup_bytes.as_ref());
    let mut archive = tar::Archive::new(dec);
    for entry in archive.entries().unwrap() {
        let mut entry = entry.unwrap();
        if entry.path().unwrap().to_str() == Some("artifacts/large.bin") {
            let mut contents = Vec::new();
            entry.read_to_end(&mut contents).unwrap();
            assert_eq!(contents.len(), payload_size);
            assert_eq!(contents, data);
            found = true;
        }
    }
    assert!(found, "large.bin should be present in the backup tarball");

    // A tarball exceeding the 2 MiB body limit should be rejected.
    let oversized = vec![0u8; 2_500_000];
    let oversized_resp = client
        .post(&restore_url)
        .bearer_auth(ADMIN_TOKEN)
        .body(oversized)
        .send()
        .await
        .unwrap();
    assert_eq!(oversized_resp.status(), StatusCode::PAYLOAD_TOO_LARGE);

    server.stop().await;
}
