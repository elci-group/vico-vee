# vico-vee

Standalone **ViCo Execution Environment** service.

Extracted from `vico-desktop` so that ViCo can talk to VEE over a well-defined HTTP API instead of embedding the executor daemon inside the desktop server.

## What it does

- Accepts sandboxed execution tasks (Python, Rust, JavaScript, Wasm, Go, Shell, Context Bundles, Osmosis).
- Enforces capability-based security with Ed25519-signed grants.
- Isolates workers via rlimits, Landlock, and seccomp-bpf.
- Persists artifacts in a content-addressable SQLite-backed store.
- Exposes a REST API for submit, status, cancel, list, artifacts, dashboard, patterns, backups, and Osmosis diff/merge/reject.
- Supports multi-tenant project isolation, rate limiting, API-key auth, TLS, and Prometheus metrics.

## Running

```bash
cd src-tauri/vico-vee
cargo run
```

The service listens on port `9987` by default. Override with `VICO_VEE_PORT`.

### Authentication

By default the service requires API-key authentication. On first run, generate
an admin key:

```bash
vico-vee --generate-admin-key
```

This writes a key file to the configured config directory (override with
`VICO_VEE_CONFIG_DIR`). Pass `--generate-admin-key /path/to/api_keys.toml` to
write to a specific location.

To allow unauthenticated access (development only), set `require_auth = false`
in your config or pass `--require-auth false`.

API keys are defined in TOML:

```toml
[keys.admin]
token = "your-secure-token"
scopes = ["submit", "read", "admin"]
```

### TLS

Provide a certificate and key to serve HTTPS:

```bash
vico-vee --tls-cert cert.pem --tls-key key.pem
```

### Backup / restore

```bash
# Create a backup tarball
vico-vee --backup --backup-output ./vee-backup.tar.gz

# Restore from a backup tarball
vico-vee --restore ./vee-backup.tar.gz
```

### Graceful shutdown

Send `SIGINT` (`Ctrl-C`). The server stops accepting new connections, waits for in-flight executions up to `shutdown_grace_period_secs`, then exits.

## Configuration

Configuration is resolved in this order (later overrides earlier):

1. Built-in defaults
2. Config file (`config.toml`/`config.yaml`/`config.yml` in `VICO_VEE_CONFIG_DIR`)
3. Environment variables
4. Command-line arguments

| Environment variable | Purpose |
|----------------------|---------|
| `VICO_VEE_PORT` | HTTP/HTTPS port (default: 9987) |
| `VICO_VEE_BIND` | Interface address (default: 0.0.0.0) |
| `VICO_VEE_DATA_DIR` | Persistent data directory |
| `VICO_VEE_CONFIG_DIR` | Configuration directory |
| `VICO_VEE_TLS_CERT` | Path to TLS certificate (PEM) |
| `VICO_VEE_TLS_KEY` | Path to TLS private key (PEM) |
| `VICO_VEE_API_KEYS_FILE` | Path to TOML API-keys file |
| `VICO_VEE_API_KEYS` | Inline TOML API-keys configuration |
| `VICO_VEE_REQUIRE_AUTH` | Require API-key auth even when no keys file exists (`true`/`false`) |
| `VICO_VEE_BODY_LIMIT_MB` | Maximum request body size in MB (default: 16) |
| `VICO_VEE_REQUEST_TIMEOUT_SECS` | Request handling timeout (default: 30) |
| `VICO_VEE_RATE_LIMIT_PER_SEC` | Per-IP rate limit (default: 10) |
| `VICO_VEE_RATE_LIMIT_BURST` | Per-IP rate-limit burst (default: 50) |
| `VICO_VEE_EXEC_RATE_LIMIT_PER_SEC` | Per-agent execution rate limit (default: 10) |
| `VICO_VEE_EXEC_RATE_LIMIT_BURST` | Per-agent execution burst (default: 30) |
| `VICO_VEE_PROJECT_RATE_LIMIT_PER_SEC` | Per-project execution rate limit (default: 10) |
| `VICO_VEE_PROJECT_RATE_LIMIT_BURST` | Per-project execution burst (default: 30) |
| `VICO_VEE_TRUSTED_PROXY_CIDRS` | Comma-separated trusted proxy CIDRs for `X-Forwarded-For` |
| `VICO_VEE_SHUTDOWN_GRACE_PERIOD_SECS` | Graceful shutdown timeout (default: 30) |
| `VICO_VEE_LOG_FORMAT` | `pretty`, `json`, or `compact` |
| `VICO_VEE_OLLAMA_URL` | Ollama endpoint used by ODIN (default: http://127.0.0.1:11434) |
| `VICO_VEE_PATTERN_STORE_PATH` | Path to the pattern store database |

## API

Public routes do not require authentication. All other routes require a valid `Authorization: Bearer <token>` header with the appropriate scope (`submit`, `read`, or `admin`).

| Method | Route | Scope | Description |
|--------|-------|-------|-------------|
| GET | `/health` | public | Service health |
| GET | `/ready` | public | Readiness probe |
| GET | `/metrics` | public | Prometheus metrics |
| GET | `/openapi.json` | public | OpenAPI specification |
| GET | `/docs` | public | API documentation |
| POST | `/admin/backup` | admin | Create a backup tarball |
| POST | `/admin/restore` | admin | Restore from a backup tarball |
| POST | `/vee/submit` | submit | Submit an execution task |
| POST | `/vee/status` | read | Get execution status |
| POST | `/vee/cancel` | submit | Cancel an execution |
| POST | `/vee/list` | read | List executions |
| POST | `/vee/artifacts` | read | Get execution artifacts |
| POST | `/vee/dashboard` | read | Dashboard statistics |
| POST | `/vee/patterns` | read | Find learned patterns |
| POST | `/vee/audit` | read | Run audit suite |
| POST | `/vee/checkpoints` | read | Checkpoint statistics |
| POST | `/vee/odin/health` | read | ODIN / Ollama health and model list |
| POST | `/vee/odin/model` | admin | Set active ODIN model |
| POST | `/vee/diff` | submit | Start Osmosis diff |
| POST | `/vee/merge` | submit | Apply Osmosis merge |
| POST | `/vee/reject` | submit | Reject Osmosis patch |

## Multi-tenancy

Pass the `x-vee-project` header to scope executions, artifacts, and dashboard statistics to a specific project. When omitted, the request belongs to the `default` project.

## ViCo integration

`vico-desktop` depends on the `vico-vee` crate and contains a `vee_client` module. The `/vee/*` routes in `vico-desktop` forward to the standalone service.

```
ViCo Desktop (React UI)
    │
    ▼
vico-desktop server
    │ HTTP /vee/*
    ▼
vico-vee service
```

## License

MIT
