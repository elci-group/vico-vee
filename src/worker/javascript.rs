//! JavaScript worker implementation.
//!
//! Executes JavaScript source code with `node` inside the sandbox.

use super::core::{verify_task_grants, RuntimeWorker, WorkerOutput};
use crate::capability::CapabilityVerifier;
use crate::sandbox::{extract_artifacts, run_sandboxed, SandboxConfig};
use crate::types::*;
use async_trait::async_trait;
use std::process::Command;
use std::sync::Arc;
use tokio::time::{timeout, Duration};

/// A worker that executes JavaScript source code with Node.js.
pub struct JavaScriptWorker {
    caps: Vec<Capability>,
    budget: ExecutionBudget,
}

impl Default for JavaScriptWorker {
    fn default() -> Self {
        Self::new()
    }
}

impl JavaScriptWorker {
    pub fn new() -> Self {
        Self {
            caps: vec![],
            budget: ExecutionBudget::default(),
        }
    }

    fn node_binary() -> String {
        std::env::var("NODE_BIN").unwrap_or_else(|_| "node".to_string())
    }
}

#[async_trait]
impl RuntimeWorker for JavaScriptWorker {
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
            return Err("JavaScript execution requires ProcessSpawn capability".into());
        }
        self.caps = caps;
        self.budget = budget;
        Ok(())
    }

    async fn execute(&self, task: &ExecutionTask) -> Result<WorkerOutput, ExecutionError> {
        if task.source_code.len() > 100_000 {
            return Err(ExecutionError {
                code: "CODE_TOO_LARGE".into(),
                message: "JavaScript source exceeds 100KB limit".into(),
                recoverable: false,
                recovery_hint: None,
            });
        }

        let node_bin = Self::node_binary();
        if !crate::worker::core::executable_present(&node_bin)
            && !std::path::PathBuf::from(&node_bin).exists()
        {
            return Err(ExecutionError {
                code: "NODE_NOT_FOUND".into(),
                message: format!("node binary not found: {}", node_bin),
                recoverable: true,
                recovery_hint: Some("Install Node.js or set NODE_BIN".into()),
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

        let script_path = work_dir.join("script.js");
        std::fs::write(&script_path, &task.source_code).map_err(|e| ExecutionError {
            code: "WRITE_FAILED".into(),
            message: e.to_string(),
            recoverable: false,
            recovery_hint: None,
        })?;

        let mut cmd = Command::new(&node_bin);
        cmd.arg(&script_path)
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
                        "JavaScript execution exceeded {}s",
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
                    "Node exited with code {:?}. stderr: {}",
                    exit_code,
                    stderr.chars().take(500).collect::<String>()
                ),
                recoverable: true,
                recovery_hint: Some("Fix script errors and retry".into()),
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
