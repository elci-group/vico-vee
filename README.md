# vico-vee

Standalone **ViCo Execution Environment** service.

Extracted from `vico-desktop` so that ViCo can talk to VEE over a well-defined HTTP API instead of embedding the executor daemon inside the desktop server.

## What it does

- Accepts sandboxed execution tasks (Python, Go, Shell, Context Bundles, Osmosis).
- Enforces capability-based security with Ed25519-signed grants.
- Isolates workers via rlimits, Landlock, and seccomp-bpf.
- Persists artifacts in a content-addressable SQLite-backed store.
- Exposes a REST API for submit, status, cancel, list, artifacts, dashboard, patterns, and Osmosis diff/merge/reject.

## Running

```bash
cd src-tauri/vico-vee
cargo run
```

The service listens on port `9987` by default. Override with `VICO_VEE_PORT`.

## Configuration

| Environment variable | Purpose |
|----------------------|---------|
| `VICO_VEE_PORT` | HTTP port (default: 9987) |
| `VICO_VEE_DATA_DIR` | Persistent data directory |
| `VICO_VEE_CONFIG_DIR` | Configuration directory |
| `VICO_VEE_KEY_PEPPER` | Pepper for fallback encrypted signing-key file |
| `VICO_VEE_OLLAMA_URL` | Ollama endpoint (reserved for future ODIN integration) |

## API

All routes accept and return JSON.

| Method | Route | Description |
|--------|-------|-------------|
| POST | `/health` | Service health |
| POST | `/vee/submit` | Submit an execution task |
| POST | `/vee/status` | Get execution status |
| POST | `/vee/cancel` | Cancel an execution |
| POST | `/vee/list` | List executions |
| POST | `/vee/artifacts` | Get execution artifacts |
| POST | `/vee/dashboard` | Dashboard statistics |
| POST | `/vee/patterns` | Find learned patterns |
| POST | `/vee/audit` | Run audit suite |
| POST | `/vee/checkpoints` | Checkpoint statistics |
| POST | `/vee/odin/health` | ODIN health (stub) |
| POST | `/vee/odin/model` | Set ODIN model (stub) |
| POST | `/vee/diff` | Start Osmosis diff |
| POST | `/vee/merge` | Apply Osmosis merge |
| POST | `/vee/reject` | Reject Osmosis patch |

## ViCo integration

`vico-desktop` now depends on the `vico-vee` crate and contains a `vee_client` module. The `/vee/*` routes in `vico-desktop` forward to the standalone service.

```
ViCo Desktop (React UI)
    │
    ▼
vico-desktop server
    │ HTTP /vee/*
    ▼
vico-vee service
```

## Migration status

- [x] Extract VEE into `vico-vee` crate
- [x] Standalone `vico-vee` binary with HTTP server
- [x] `vico-desktop` HTTP client (`vee_client`)
- [x] `/vee/*` routes in `vico-desktop` forwarded to `vico-vee`
- [x] CED pipeline uses `vee_client`
- [x] Background daemon migrated to `vee_client`
- [x] Antigravity adapter migrated to `vee_client`
- [x] Eval harness migrated to `vee_client`
- [ ] Remove embedded `ExecutorDaemon` from `vico-desktop` once audit/checkpoint/ODIN endpoints migrate

## License

MIT
