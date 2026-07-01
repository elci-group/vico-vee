# vico-vee Benchmarks

This document records baseline performance numbers for the standalone `vico-vee` service.

## Running Benchmarks

From the `src-tauri` workspace root:

```bash
cargo bench -p vico-vee
```

Results are written to `target/criterion/` and can be viewed in a browser:

```bash
# On most distributions
xdg-open target/criterion/report/index.html

# On macOS
open target/criterion/report/index.html
```

## Environment

Numbers below were collected on the reference machine used during development. Re-run benchmarks on your target hardware for accurate planning.

| Property | Value |
|---|---|
| CPU | Intel Core 5 120U (12 threads) |
| RAM | 8 GiB |
| OS | Linux pop-os 6.18.7 x86_64 |
| Rust version | rustc 1.96.0 |
| Date | 2026-07-02 |

## Workloads

### `noop_python_submit/1000_tasks`

Submits 1000 trivial shell tasks (`echo noop`) concurrently across 10 clients and measures end-to-end submission latency. The workload exercises the HTTP stack, auth middleware, rate-limiting, task queue, and sandbox spawn/teardown.

| Metric | Value |
|---|---|
| Samples | 10 |
| Tasks per iteration | 1000 |
| Mean batch time | ~16.0 s |
| Criterion throughput | ~62.5 tasks/s |
| p50 latency | 125.4 ms |
| p99 latency | 871.0 ms |

> **Note:** The benchmark intentionally uses `shell` rather than `python` to avoid Python interpreter availability skewing results across environments.

## Load Tests

Integration load tests live in `tests/load.rs` and can be run with:

```bash
cargo test -p vico-vee --test load -- --nocapture
```

They verify:

- 20 concurrent CPU/sleep tasks reach a terminal status.
- Graceful shutdown under load does not lose terminal statuses for submitted executions.
- Large artifact upload/download round-trips correctly and respects the body limit.

## Interpreting Results

- **p50 latency** — typical user-visible submit latency.
- **p99 latency** — tail latency under concurrent load; sensitive to sandbox/seccomp and filesystem contention.
- **Throughput** — approximate tasks/s assuming independent submissions.

For production sizing, run benchmarks on the target OS and hardware, then set rate limits (`rate_limit.per_sec`, `rate_limit.burst`, `rate_limit.exec_per_sec`, `rate_limit.exec_burst`) to stay below 70% of measured sustainable throughput.
