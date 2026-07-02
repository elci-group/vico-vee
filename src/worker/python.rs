//! Python worker implementation.
//!
//! Executes Python source code inside a sandboxed subprocess with Landlock
//! and seccomp-bpf protections.

use super::core::{verify_task_grants, RuntimeWorker, WorkerOutput};
use crate::capability::CapabilityVerifier;
use crate::sandbox::{build_python_command, extract_artifacts, run_sandboxed};
use crate::types::*;
use async_trait::async_trait;
use std::sync::Arc;
use tokio::time::{timeout, Duration};

/// A real Python worker that executes code in a sandboxed subprocess.
pub struct PythonWorker {
    caps: Vec<Capability>,
    budget: ExecutionBudget,
}

impl Default for PythonWorker {
    fn default() -> Self {
        Self::new()
    }
}

impl PythonWorker {
    pub fn new() -> Self {
        Self {
            caps: vec![],
            budget: ExecutionBudget::default(),
        }
    }
}

#[async_trait]
impl RuntimeWorker for PythonWorker {
    async fn init(
        &mut self,
        execution_id: &str,
        grants: Vec<CapabilityGrant>,
        verifier: Arc<CapabilityVerifier>,
        required_capabilities: Vec<Capability>,
        budget: ExecutionBudget,
    ) -> Result<(), String> {
        self.caps = verify_task_grants(execution_id, &grants, &verifier, &required_capabilities)?;
        self.budget = budget;
        Ok(())
    }

    async fn execute(&self, task: &ExecutionTask) -> Result<WorkerOutput, ExecutionError> {
        // Validate code size
        if task.source_code.len() > 50_000 {
            return Err(ExecutionError {
                code: "CODE_TOO_LARGE".into(),
                message: "Source code exceeds 50KB limit".into(),
                recoverable: false,
                recovery_hint: None,
            });
        }

        // Build temp directories
        let base_dir = tempfile::tempdir().map_err(|e| ExecutionError {
            code: "SANDBOX_SETUP_FAILED".into(),
            message: format!("Failed to create temp dir: {}", e),
            recoverable: true,
            recovery_hint: Some("Check temp directory permissions".into()),
        })?;
        let work_dir = base_dir.path().join("work");
        let output_dir = base_dir.path().join("output");

        // Build command and config
        let (cmd, mut config) =
            match build_python_command(&task.source_code, &work_dir, &output_dir) {
                Ok(v) => v,
                Err(e) => {
                    return Err(ExecutionError {
                        code: "SANDBOX_SETUP_FAILED".into(),
                        message: e,
                        recoverable: true,
                        recovery_hint: Some("Check temp directory permissions".into()),
                    })
                }
            };

        // Apply budget and capabilities to config
        config.budget = self.budget.clone();
        config.block_network = !self
            .caps
            .iter()
            .any(|c| matches!(c, Capability::NetworkAccess { .. } | Capability::NetworkDns));

        // Run with wall-clock timeout
        let exec_future = tokio::task::spawn_blocking(move || run_sandboxed(cmd, &config));

        let sandbox_result = match timeout(
            Duration::from_secs(self.budget.wall_clock_seconds.max(1)),
            exec_future,
        )
        .await
        {
            Ok(Ok(Ok(result))) => result,
            Ok(Ok(Err(e))) => {
                return Err(ExecutionError {
                    code: "SANDBOX_ERROR".into(),
                    message: e,
                    recoverable: true,
                    recovery_hint: Some("Sandbox setup failed — check kernel support".into()),
                });
            }
            Ok(Err(e)) => {
                return Err(ExecutionError {
                    code: "TASK_PANIC".into(),
                    message: format!("Worker thread panicked: {}", e),
                    recoverable: false,
                    recovery_hint: None,
                });
            }
            Err(_) => {
                return Err(ExecutionError {
                    code: "TIMEOUT".into(),
                    message: format!(
                        "Execution exceeded {}s wall-clock limit",
                        self.budget.wall_clock_seconds
                    ),
                    recoverable: true,
                    recovery_hint: Some("Increase wall_clock_seconds budget".into()),
                });
            }
        };

        // Check exit code
        if let Some(code) = sandbox_result.exit_code {
            if code != 0 {
                return Err(ExecutionError {
                    code: "EXIT_CODE".into(),
                    message: format!(
                        "Process exited with code {}. stderr: {}",
                        code,
                        sandbox_result.stderr.chars().take(500).collect::<String>()
                    ),
                    recoverable: true,
                    recovery_hint: Some("Fix code errors and retry".into()),
                });
            }
        }

        // Extract artifacts
        let mut artifacts = extract_artifacts(&sandbox_result, &output_dir);

        // Add metadata artifact
        let mut meta = serde_json::Map::new();
        meta.insert("duration_ms".into(), sandbox_result.duration_ms.into());
        meta.insert(
            "memory_peak_kb".into(),
            sandbox_result.memory_peak_kb.into(),
        );
        meta.insert(
            "sandbox_layers".into(),
            sandbox_result.sandbox_layers_applied.into(),
        );
        if !sandbox_result.sandbox_errors.is_empty() {
            meta.insert(
                "sandbox_errors".into(),
                sandbox_result.sandbox_errors.into(),
            );
        }

        artifacts.push(Artifact::Json {
            value: serde_json::Value::Object(meta),
            schema_hash: "sandbox-meta".into(),
        });

        Ok(WorkerOutput {
            artifacts,
            stderr: sandbox_result.stderr,
            exit_code: sandbox_result.exit_code,
        })
    }

    async fn shutdown(self: Box<Self>) -> Result<(), String> {
        Ok(())
    }
}
