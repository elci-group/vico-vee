//! Criterion benchmark for `vico-vee` task submission throughput and latency.

use criterion::{black_box, criterion_group, criterion_main, Criterion, Throughput};
use reqwest::Client;
use serde_json::{json, Value};
use std::net::SocketAddr;
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tempfile::TempDir;
use tokio::net::TcpListener;
use tokio_util::sync::CancellationToken;
use vico_vee::config::{ApiKeysConfig, Config, RateLimitConfig};
use vico_vee::server::{router, AppState};

const ADMIN_TOKEN: &str = "bench-admin-token";
const API_KEYS_TOML: &str = r#"
[keys.admin]
token = "bench-admin-token"
scopes = ["submit", "read", "admin"]
"#;

fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() - 1) as f64 * p).floor() as usize;
    sorted[idx]
}

async fn submit_noop(client: &Client, addr: &SocketAddr, seq: usize) {
    let url = format!("http://{}/vee/submit", addr);
    let body = json!({
        "agent_id": format!("bench-agent-{seq}"),
        "language": "python",
        "source_code": "print(\"noop\")",
        "capabilities": ["process_spawn"],
    });
    let resp = client
        .post(&url)
        .bearer_auth(ADMIN_TOKEN)
        .json(&body)
        .send()
        .await
        .unwrap();
    assert!(resp.status().is_success());
    black_box(resp);
}

fn bench_submit_1000_noop_python(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    let tmp = TempDir::new().unwrap();
    let data_dir = tmp.path().join("data");
    let config_dir = tmp.path().join("config");
    std::fs::create_dir_all(&config_dir).unwrap();
    let keys_path = config_dir.join("api_keys.toml");
    std::fs::write(&keys_path, API_KEYS_TOML).unwrap();

    let config = Config {
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
            per_sec: 10_000,
            burst: 10_000,
            exec_per_sec: 10_000,
            exec_burst: 10_000,
        },
        shutdown_grace_period_secs: 30,
        ..Config::default()
    };

    let state = rt
        .block_on(AppState::try_new(config.clone()))
        .expect("failed to create AppState");
    let app = router(state.clone());

    let listener = rt
        .block_on(TcpListener::bind((config.bind.as_str(), 0)))
        .unwrap();
    let addr = listener.local_addr().unwrap();

    let shutdown = CancellationToken::new();
    let shutdown_clone = shutdown.clone();
    let server_handle = rt.spawn(async move {
        axum::serve(listener, app)
            .with_graceful_shutdown(shutdown_clone.cancelled_owned())
            .await
            .unwrap();
    });
    std::thread::sleep(Duration::from_millis(50));

    let client = Client::new();

    // Prime a single submission so any one-time setup is not part of the
    // measured iterations.
    rt.block_on(submit_noop(&client, &addr, 0));

    static LATENCIES: Mutex<Vec<Duration>> = Mutex::new(Vec::new());

    let mut group = c.benchmark_group("noop_python_submit");
    group.throughput(Throughput::Elements(1000));
    group.warm_up_time(Duration::from_secs(3));
    group.sample_size(10);
    group.measurement_time(Duration::from_secs(300));

    group.bench_function("1000_tasks", |b| {
        b.iter(|| {
            let mut iteration_latencies = Vec::with_capacity(1000);
            rt.block_on(async {
                for i in 0..1000 {
                    let start = Instant::now();
                    submit_noop(&client, &addr, i + 1).await;
                    iteration_latencies.push(start.elapsed());
                }
            });
            LATENCIES.lock().unwrap().extend(iteration_latencies);
        });
    });
    group.finish();

    shutdown.cancel();
    let _ = rt.block_on(tokio::time::timeout(Duration::from_secs(5), server_handle));
    rt.block_on(state.vee.stop());

    let mut all: Vec<f64> = LATENCIES
        .lock()
        .unwrap()
        .iter()
        .map(|d| d.as_secs_f64() * 1000.0)
        .collect();
    if !all.is_empty() {
        all.sort_unstable_by(|a, b| a.partial_cmp(b).unwrap());
        let p50 = percentile(&all, 0.5);
        let p99 = percentile(&all, 0.99);
        let throughput = if p50 > 0.0 { 1000.0 / p50 } else { 0.0 };
        eprintln!(
            "\nvico-vee noop Python submit distribution (N={}): p50={:.3}ms, p99={:.3}ms, approx throughput={:.1} tasks/ms",
            all.len(), p50, p99, throughput
        );
    }
}

criterion_group!(benches, bench_submit_1000_noop_python);
criterion_main!(benches);
