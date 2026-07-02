//! WebAssembly worker implementation.
//!
//! Expects task source_code to be base64-encoded WASM module bytes and runs the
//! module with `wasmtime` inside the sandbox.

use super::core::{verify_task_grants, RuntimeWorker, WorkerOutput};
use crate::capability::CapabilityVerifier;
use crate::sandbox::{extract_artifacts, run_sandboxed, SandboxConfig};
use crate::types::*;
use async_trait::async_trait;
use std::process::Command;
use std::sync::Arc;
use tokio::time::{timeout, Duration};

/// A worker that runs a base64-encoded WebAssembly module with wasmtime.
pub struct WasmWorker {
    caps: Vec<Capability>,
    budget: ExecutionBudget,
}

impl Default for WasmWorker {
    fn default() -> Self {
        Self::new()
    }
}

impl WasmWorker {
    pub fn new() -> Self {
        Self {
            caps: vec![],
            budget: ExecutionBudget::default(),
        }
    }

    fn wasmtime_binary() -> String {
        std::env::var("WASMTIME_BIN").unwrap_or_else(|_| "wasmtime".to_string())
    }
}

#[async_trait]
impl RuntimeWorker for WasmWorker {
    async fn init(
        &mut self,
        execution_id: &str,
        grants: Vec<CapabilityGrant>,
        verifier: Arc<CapabilityVerifier>,
        required_capabilities: Vec<Capability>,
        budget: ExecutionBudget,
    ) -> Result<(), String> {
        let caps = verify_task_grants(execution_id, &grants, &verifier, &required_capabilities)?;
        if !caps.iter().any(|c| matches!(c, Capability::ProcessSpawn)) {
            return Err("Wasm execution requires ProcessSpawn capability".into());
        }
        self.caps = caps;
        self.budget = budget;
        Ok(())
    }

    async fn execute(&self, task: &ExecutionTask) -> Result<WorkerOutput, ExecutionError> {
        let wasmtime_bin = Self::wasmtime_binary();
        if !crate::worker::core::executable_present(&wasmtime_bin)
            && !std::path::PathBuf::from(&wasmtime_bin).exists()
        {
            return Err(ExecutionError {
                code: "WASMTIME_NOT_FOUND".into(),
                message: format!("wasmtime binary not found: {}", wasmtime_bin),
                recoverable: true,
                recovery_hint: Some("Install wasmtime or set WASMTIME_BIN".into()),
            });
        }

        let base_dir = tempfile::tempdir().map_err(|e| ExecutionError {
            code: "SANDBOX_SETUP_FAILED".into(),
            message: format!("Failed to create temp dir: {}", e),
            recoverable: true,
            recovery_hint: None,
        })?;
        let work_dir = base_dir.path().join("work");
        let output_dir = base_dir.path().join("output");
        std::fs::create_dir_all(&work_dir).map_err(|e| ExecutionError {
            code: "SANDBOX_SETUP_FAILED".into(),
            message: format!("Failed to create work dir: {}", e),
            recoverable: true,
            recovery_hint: None,
        })?;
        std::fs::create_dir_all(&output_dir).map_err(|e| ExecutionError {
            code: "SANDBOX_SETUP_FAILED".into(),
            message: format!("Failed to create output dir: {}", e),
            recoverable: true,
            recovery_hint: None,
        })?;

        let wasm_bytes = base64::Engine::decode(
            &base64::engine::general_purpose::STANDARD,
            &task.source_code,
        )
        .map_err(|e| ExecutionError {
            code: "INVALID_WASM_BASE64".into(),
            message: format!("Failed to decode base64 wasm source: {}", e),
            recoverable: false,
            recovery_hint: Some("Provide base64-encoded WASM module bytes".into()),
        })?;

        let module_path = work_dir.join("module.wasm");
        std::fs::write(&module_path, wasm_bytes).map_err(|e| ExecutionError {
            code: "WRITE_FAILED".into(),
            message: e.to_string(),
            recoverable: false,
            recovery_hint: None,
        })?;

        let mut cmd = Command::new(&wasmtime_bin);
        cmd.arg(&module_path)
            .current_dir(&work_dir)
            .env_clear()
            .env("HOME", &work_dir)
            .env("PATH", "/usr/local/bin:/usr/bin:/bin");

        let config = SandboxConfig {
            work_dir: work_dir.clone(),
            output_dir: output_dir.clone(),
            input_paths: vec![],
            executable_paths: vec![],
            budget: self.budget.clone(),
            capabilities: self.caps.clone(),
            block_network: !self
                .caps
                .iter()
                .any(|c| matches!(c, Capability::NetworkAccess { .. })),
        };

        let exec_future = tokio::task::spawn_blocking(move || run_sandboxed(cmd, &config));
        let result = match timeout(
            Duration::from_secs(self.budget.wall_clock_seconds.max(1)),
            exec_future,
        )
        .await
        {
            Ok(Ok(Ok(r))) => r,
            Ok(Ok(Err(e))) => {
                return Err(ExecutionError {
                    code: "SANDBOX_ERROR".into(),
                    message: e,
                    recoverable: true,
                    recovery_hint: None,
                })
            }
            Ok(Err(_)) => {
                return Err(ExecutionError {
                    code: "TASK_PANIC".into(),
                    message: "Worker thread panicked".into(),
                    recoverable: false,
                    recovery_hint: None,
                })
            }
            Err(_) => {
                return Err(ExecutionError {
                    code: "TIMEOUT".into(),
                    message: format!(
                        "Wasm execution exceeded {}s",
                        self.budget.wall_clock_seconds
                    ),
                    recoverable: true,
                    recovery_hint: Some("Increase wall_clock_seconds budget".into()),
                })
            }
        };

        let stderr = result.stderr.clone();
        let exit_code = result.exit_code;

        if exit_code != Some(0) {
            return Err(ExecutionError {
                code: "EXIT_CODE".into(),
                message: format!(
                    "wasmtime exited with code {:?}. stderr: {}",
                    exit_code,
                    stderr.chars().take(500).collect::<String>()
                ),
                recoverable: true,
                recovery_hint: Some("Check wasm module and retry".into()),
            });
        }

        let mut artifacts = extract_artifacts(&result, &output_dir);
        let mut meta = serde_json::Map::new();
        meta.insert("duration_ms".into(), result.duration_ms.into());
        meta.insert("memory_peak_kb".into(), result.memory_peak_kb.into());
        artifacts.push(Artifact::Json {
            value: serde_json::Value::Object(meta),
            schema_hash: "sandbox-meta".into(),
        });

        Ok(WorkerOutput {
            artifacts,
            stderr,
            exit_code,
        })
    }

    async fn shutdown(self: Box<Self>) -> Result<(), String> {
        Ok(())
    }
}
