# vico-vee Roadmap to SOTA

This roadmap tracks the work required to move `vico-vee` from a solid
skeleton to a production-grade, state-of-the-art sandboxed execution service.

## Phase 1 — Security, Safety & Correctness (completed)

Goal: close critical production blockers and make the service safe to expose.

- [x] Fix `scripts/install.sh` API-key format to match the TOML schema expected by
      `src/auth.rs` (`[keys.admin] token = "..." scopes = [...]`).
- [x] Fail closed on authentication: when no API keys are configured and auth is
      required, the service refuses to start instead of silently disabling auth.
- [x] Add `--generate-admin-key` CLI flag for ergonomic first-run key creation.
- [x] Enforce a global request body limit via `tower_http::RequestBodyLimitLayer`
      using `config.body_limit_mb`.
- [x] Fix HTTPS `ConnectInfo` so per-IP rate limiting works over TLS.
- [x] Implement real graceful shutdown on `SIGINT`/`SIGTERM` that waits for
      in-flight executions up to `shutdown_grace_period_secs`.
- [x] Persist execution metadata to SQLite (`vee_executions.db`) and restore it
      on daemon startup.
- [x] Add meaningful Prometheus metrics: request count, latency histogram, and
      per-route/status counters, plus existing execution gauges.
- [x] Make `/ready` check both daemon liveness and SQLite connectivity.
- [x] Add integration tests for body limits, execution persistence across
      restarts, and request metrics.

## Phase 2 — Completeness & Operability

Goal: fill functional gaps and improve operational experience.

- [ ] Implement or remove Rust / JavaScript / Wasm worker stubs.
- [ ] Implement ODIN integration or remove the stub endpoints from the public API.
- [ ] Invoke hypothesis validation in `daemon/runner.rs` and surface results.
- [ ] Persist patterns via `PatternStore::new_with_path` instead of in-memory.
- [ ] Add request-timeout middleware using `config.request_timeout_secs`.
- [ ] Add per-project rate limits and quotas.
- [ ] Add `X-Forwarded-For` / trusted-proxy IP extraction for rate limiting.
- [ ] Complete OpenAPI request/response schemas and regenerate docs.
- [ ] Harden Dockerfile (`HEALTHCHECK`, example configs) and add
      `docker-compose.yml`.
- [ ] Add CI/CD workflow with `cargo audit`, `clippy::pedantic`, and release
      builds.

## Phase 3 — SOTA Hardening & DX

Goal: reach state-of-the-art security, observability, and developer experience.

- [ ] Enforce capability path scopes in Landlock / seccomp filters.
- [ ] Block network access when `block_network` is true (network namespaces or
      seccomp network syscalls).
- [ ] Add OpenTelemetry / Jaeger tracing export.
- [ ] Add chaos/fuzz tests for sandbox escapes (filesystem, network, process).
- [ ] Replace `String` errors with typed `thiserror` enums on the public API.
- [ ] Add architecture documentation, threat model, and capability semantics.
- [ ] Add a Helm chart / Kubernetes manifests for clustered deployment.
- [ ] Support live configuration reload without restart.
