//! Rust worker implementation.
//!
//! Compiles Rust source code with `rustc` and runs the resulting binary inside
//! the sandbox.

use super::core::{verify_task_grants, RuntimeWorker, WorkerOutput};
use crate::capability::CapabilityVerifier;
use crate::sandbox::{extract_artifacts, run_sandboxed, SandboxConfig};
use crate::types::*;
use async_trait::async_trait;
use std::process::Command;
use std::sync::Arc;
use tokio::time::{timeout, Duration};

/// A worker that compiles and executes Rust source code.
pub struct RustWorker {
    caps: Vec<Capability>,
    budget: ExecutionBudget,
}

impl Default for RustWorker {
    fn default() -> Self {
        Self::new()
    }
}

impl RustWorker {
    pub fn new() -> Self {
        Self {
            caps: vec![],
            budget: ExecutionBudget::default(),
        }
    }

    fn rustc_binary() -> String {
        std::env::var("RUSTC_BIN").unwrap_or_else(|_| "rustc".to_string())
    }
}

#[async_trait]
impl RuntimeWorker for RustWorker {
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
            return Err("Rust execution requires ProcessSpawn capability".into());
        }
        self.caps = caps;
        self.budget = budget;
        Ok(())
    }

    async fn execute(&self, task: &ExecutionTask) -> Result<WorkerOutput, ExecutionError> {
        if task.source_code.len() > 100_000 {
            return Err(ExecutionError {
                code: "CODE_TOO_LARGE".into(),
                message: "Rust source exceeds 100KB limit".into(),
                recoverable: false,
                recovery_hint: None,
            });
        }

        let rustc_bin = Self::rustc_binary();
        if !crate::worker::core::executable_present(&rustc_bin)
            && !std::path::PathBuf::from(&rustc_bin).exists()
        {
            return Err(ExecutionError {
                code: "RUSTC_NOT_FOUND".into(),
                message: format!("rustc binary not found: {}", rustc_bin),
                recoverable: true,
                recovery_hint: Some("Install rustc or set RUSTC_BIN".into()),
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

        let source_path = work_dir.join("main.rs");
        std::fs::write(&source_path, &task.source_code).map_err(|e| ExecutionError {
            code: "WRITE_FAILED".into(),
            message: e.to_string(),
            recoverable: false,
            recovery_hint: None,
        })?;

        let binary_path = work_dir.join("main");

        // Compile
        let mut compile_cmd = Command::new(&rustc_bin);
        compile_cmd
            .arg("-O")
            .arg(&source_path)
            .arg("-o")
            .arg(&binary_path)
            .current_dir(&work_dir)
            .env_clear()
            .env("HOME", &work_dir)
            .env("PATH", "/usr/local/bin:/usr/bin:/bin");

        let compile_config = SandboxConfig {
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

        let compile_future =
            tokio::task::spawn_blocking(move || run_sandboxed(compile_cmd, &compile_config));
        let compile_result = match timeout(
            Duration::from_secs(self.budget.wall_clock_seconds.max(1)),
            compile_future,
        )
        .await
        {
            Ok(Ok(Ok(r))) => r,
            Ok(Ok(Err(e))) => {
                return Err(ExecutionError {
                    code: "COMPILE_ERROR".into(),
                    message: e,
                    recoverable: true,
                    recovery_hint: Some("Check Rust syntax and available crates".into()),
                })
            }
            Ok(Err(_)) => {
                return Err(ExecutionError {
                    code: "TASK_PANIC".into(),
                    message: "Compiler thread panicked".into(),
                    recoverable: false,
                    recovery_hint: None,
                })
            }
            Err(_) => {
                return Err(ExecutionError {
                    code: "COMPILE_TIMEOUT".into(),
                    message: format!(
                        "Rust compilation exceeded {}s",
                        self.budget.wall_clock_seconds
                    ),
                    recoverable: true,
                    recovery_hint: Some("Increase wall_clock_seconds budget".into()),
                })
            }
        };

        if compile_result.exit_code != Some(0) {
            return Err(ExecutionError {
                code: "COMPILATION_FAILED".into(),
                message: format!(
                    "rustc exited with code {:?}. stderr: {}",
                    compile_result.exit_code,
                    compile_result.stderr.chars().take(700).collect::<String>()
                ),
                recoverable: true,
                recovery_hint: Some("Fix compilation errors and retry".into()),
            });
        }

        // Run compiled binary
        let mut run_cmd = Command::new(&binary_path);
        run_cmd
            .current_dir(&work_dir)
            .env_clear()
            .env("HOME", &work_dir)
            .env("PATH", "/usr/local/bin:/usr/bin:/bin");

        let run_config = SandboxConfig {
            work_dir: work_dir.clone(),
            output_dir: output_dir.clone(),
            input_paths: vec![],
            executable_paths: vec![binary_path.clone()],
            budget: self.budget.clone(),
            capabilities: self.caps.clone(),
            block_network: !self
                .caps
                .iter()
                .any(|c| matches!(c, Capability::NetworkAccess { .. })),
        };

        let run_future = tokio::task::spawn_blocking(move || run_sandboxed(run_cmd, &run_config));
        let result = match timeout(
            Duration::from_secs(self.budget.wall_clock_seconds.max(1)),
            run_future,
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
                    message: format!("Rust execution exceeded {}s", self.budget.wall_clock_seconds),
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
                    "Rust binary exited with code {:?}. stderr: {}",
                    exit_code,
                    stderr.chars().take(500).collect::<String>()
                ),
                recoverable: true,
                recovery_hint: Some("Fix runtime errors and retry".into()),
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
