//! Shell worker implementation.
//!
//! Executes shell scripts inside a sandboxed subprocess with tightened
//! environment and capability restrictions.

use super::core::{verify_task_grants, RuntimeWorker};
use crate::capability::CapabilityVerifier;
use crate::sandbox::{extract_artifacts, run_sandboxed, SandboxConfig};
use crate::types::*;
use async_trait::async_trait;
use std::process::Command;
use std::sync::Arc;
use tokio::time::{timeout, Duration};

/// A real Shell worker with extra restrictions.
pub struct ShellWorker {
    caps: Vec<Capability>,
    budget: ExecutionBudget,
}

impl Default for ShellWorker {
    fn default() -> Self {
        Self::new()
    }
}

impl ShellWorker {
    pub fn new() -> Self {
        Self {
            caps: vec![],
            budget: ExecutionBudget::default(),
        }
    }
}

#[async_trait]
impl RuntimeWorker for ShellWorker {
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
            return Err("Shell execution requires ProcessSpawn capability".into());
        }
        self.caps = caps;
        self.budget = budget;
        Ok(())
    }

    async fn execute(&self, task: &ExecutionTask) -> Result<Vec<Artifact>, ExecutionError> {
        if task.source_code.len() > 50_000 {
            return Err(ExecutionError {
                code: "CODE_TOO_LARGE".into(),
                message: "Script exceeds 50KB limit".into(),
                recoverable: false,
                recovery_hint: None,
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

        let script_path = work_dir.join("script.sh");
        std::fs::write(&script_path, &task.source_code).map_err(|e| ExecutionError {
            code: "WRITE_FAILED".into(),
            message: e.to_string(),
            recoverable: false,
            recovery_hint: None,
        })?;

        let mut cmd = Command::new("bash");
        cmd.arg(&script_path)
            .current_dir(&work_dir)
            .env_clear()
            .env("HOME", &work_dir)
            .env("PATH", "/usr/bin:/bin");

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
                        "Shell execution exceeded {}s",
                        self.budget.wall_clock_seconds
                    ),
                    recoverable: true,
                    recovery_hint: Some("Increase wall_clock_seconds budget".into()),
                })
            }
        };

        if let Some(code) = result.exit_code {
            if code != 0 {
                return Err(ExecutionError {
                    code: "EXIT_CODE".into(),
                    message: format!(
                        "Shell exited with code {}. stderr: {}",
                        code,
                        result.stderr.chars().take(500).collect::<String>()
                    ),
                    recoverable: true,
                    recovery_hint: Some("Fix script errors and retry".into()),
                });
            }
        }

        let mut artifacts = extract_artifacts(&result, &output_dir);
        let mut meta = serde_json::Map::new();
        meta.insert("duration_ms".into(), result.duration_ms.into());
        meta.insert("memory_peak_kb".into(), result.memory_peak_kb.into());
        artifacts.push(Artifact::Json {
            value: serde_json::Value::Object(meta),
            schema_hash: "sandbox-meta".into(),
        });

        Ok(artifacts)
    }

    async fn shutdown(self: Box<Self>) -> Result<(), String> {
        Ok(())
    }
}
