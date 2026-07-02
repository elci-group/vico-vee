//! Go and context-bundle worker implementations.
//!
//! Both workers delegate to external CLI tooling (`goten`, `egor`, `bound`)
//! and share helpers for resolving tool binaries.

use super::core::{executable_present, tool_binary, verify_task_grants, RuntimeWorker, WorkerOutput};
use crate::capability::CapabilityVerifier;
use crate::types::*;
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::process::Command as TokioCommand;
use tokio::time::{timeout, Duration};

/// A Go worker that delegates hermetic module execution to goten.
///
/// The task source is treated as goten/go command arguments, for example:
/// `test ./...`, `build ./cmd/app`, or `goten --isolated test ./...`.
pub struct GoWorker {
    caps: Vec<Capability>,
    budget: ExecutionBudget,
}

impl Default for GoWorker {
    fn default() -> Self {
        Self::new()
    }
}

impl GoWorker {
    pub fn new() -> Self {
        Self {
            caps: vec![],
            budget: ExecutionBudget::default(),
        }
    }

    fn module_dir(&self) -> Result<PathBuf, ExecutionError> {
        let path = self
            .caps
            .iter()
            .find_map(|cap| match cap {
                Capability::FilesystemRead { paths } => paths.first().cloned(),
                _ => None,
            })
            .or_else(|| std::env::var("VICO_WORKSPACE_DIR").ok())
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        if !path.exists() {
            return Err(ExecutionError {
                code: "GO_MODULE_DIR_MISSING".into(),
                message: format!("Go module directory does not exist: {}", path.display()),
                recoverable: true,
                recovery_hint: Some(
                    "Grant filesystem_read:/absolute/path/to/go/module for Go VEE tasks".into(),
                ),
            });
        }

        Ok(path)
    }

    fn goten_args(source_code: &str) -> Vec<String> {
        let command = source_code
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with('#'))
            .unwrap_or("test ./...")
            .trim_start_matches('$')
            .trim();

        let command = command
            .strip_prefix("goten ")
            .or_else(|| command.strip_prefix("go "))
            .unwrap_or(command);

        command.split_whitespace().map(ToOwned::to_owned).collect()
    }
}

#[async_trait]
impl RuntimeWorker for GoWorker {
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
            return Err("Go execution requires ProcessSpawn capability".into());
        }
        if !caps
            .iter()
            .any(|c| matches!(c, Capability::FilesystemRead { .. }))
        {
            return Err("Go execution requires FilesystemRead capability".into());
        }

        self.caps = caps;
        self.budget = budget;
        Ok(())
    }

    async fn execute(&self, task: &ExecutionTask) -> Result<WorkerOutput, ExecutionError> {
        if task.source_code.len() > 8_000 {
            return Err(ExecutionError {
                code: "COMMAND_TOO_LARGE".into(),
                message: "Go command source exceeds 8KB limit".into(),
                recoverable: false,
                recovery_hint: None,
            });
        }

        let module_dir = self.module_dir()?;
        let goten_bin = tool_binary("GOTEN_BIN", "goten", "goten");
        let args = Self::goten_args(&task.source_code);
        let wall_clock = self.budget.wall_clock_seconds.max(1);
        let goten_present = executable_present(&goten_bin) || PathBuf::from(&goten_bin).exists();
        let egor_bin = tool_binary("EGOR_BIN", "egor", "egor");
        let egor_present = executable_present(&egor_bin) || PathBuf::from(&egor_bin).exists();
        let started = std::time::Instant::now();

        let mut command = TokioCommand::new(&goten_bin);
        command
            .args(&args)
            .current_dir(&module_dir)
            .kill_on_drop(true)
            .env_clear()
            .env("PATH", "/usr/local/bin:/usr/bin:/bin")
            .env("GOTOOLCHAIN", "auto");

        let output = match timeout(Duration::from_secs(wall_clock), command.output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => {
                return Err(ExecutionError {
                    code: "GOTEN_SPAWN_FAILED".into(),
                    message: format!("Failed to execute goten: {}", e),
                    recoverable: true,
                    recovery_hint: Some(
                        "Build elci-group/goten and place it on PATH or set GOTEN_BIN".into(),
                    ),
                })
            }
            Err(_) => {
                return Err(ExecutionError {
                    code: "TIMEOUT".into(),
                    message: format!("Go execution exceeded {}s", wall_clock),
                    recoverable: true,
                    recovery_hint: Some(
                        "Increase wall_clock_seconds or narrow the Go command".into(),
                    ),
                })
            }
        };

        let duration_ms = started.elapsed().as_millis() as u64;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let code = output.status.code().unwrap_or(-1);

        if !output.status.success() {
            return Err(ExecutionError {
                code: "GOTEN_EXIT_CODE".into(),
                message: format!(
                    "goten exited with code {}. stderr: {}",
                    code,
                    stderr.chars().take(700).collect::<String>()
                ),
                recoverable: true,
                recovery_hint: Some("Inspect goten diagnostics, then retry through VEE".into()),
            });
        }

        let mut artifacts = Vec::new();
        artifacts.push(Artifact::Text {
            content: stdout.clone(),
            format: TextFormat::Plain,
            line_count: stdout.lines().count(),
        });

        if !stderr.trim().is_empty() {
            let mut level_counts = std::collections::HashMap::new();
            level_counts.insert(LogLevel::Warn, stderr.lines().count());
            artifacts.push(Artifact::Log {
                entries: stderr
                    .lines()
                    .map(|message| LogEntry {
                        timestamp: chrono::Utc::now(),
                        level: LogLevel::Warn,
                        message: message.to_string(),
                        source: "goten".into(),
                    })
                    .collect(),
                level_counts,
            });
        }

        artifacts.push(Artifact::Json {
            value: serde_json::json!({
                "worker": "go",
                "executor": "goten",
                "provisioner": "egor",
                "module_dir": module_dir,
                "command": args,
                "exit_code": code,
                "duration_ms": duration_ms,
                "goten_present": goten_present,
                "egor_present": egor_present,
                "ai_diagnostics_env_forwarded": false
            }),
            schema_hash: "vee-tool-contract-go-v1".into(),
        });

        Ok(WorkerOutput {
            artifacts,
            stderr,
            exit_code: Some(code),
        })
    }

    async fn shutdown(self: Box<Self>) -> Result<(), String> {
        Ok(())
    }
}

/// A context-bundling worker backed by the elci-group/bound CLI.
///
/// The task source is treated as bound arguments, for example:
/// `[rs] . --tree --meta -d 3 -t 1200`.
pub struct ContextBundleWorker {
    caps: Vec<Capability>,
    budget: ExecutionBudget,
}

impl Default for ContextBundleWorker {
    fn default() -> Self {
        Self::new()
    }
}

impl ContextBundleWorker {
    pub fn new() -> Self {
        Self {
            caps: vec![],
            budget: ExecutionBudget::default(),
        }
    }

    fn root_dir(&self) -> Result<PathBuf, ExecutionError> {
        let path = self
            .caps
            .iter()
            .find_map(|cap| match cap {
                Capability::FilesystemRead { paths } => paths.first().cloned(),
                _ => None,
            })
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        if !path.exists() {
            return Err(ExecutionError {
                code: "BUNDLE_ROOT_MISSING".into(),
                message: format!("Context bundle root does not exist: {}", path.display()),
                recoverable: true,
                recovery_hint: Some(
                    "Grant filesystem_read:/absolute/path/to/repository for context bundles".into(),
                ),
            });
        }

        Ok(path)
    }

    fn bound_args(source_code: &str) -> Vec<String> {
        let command = source_code
            .lines()
            .map(str::trim)
            .find(|line| !line.is_empty() && !line.starts_with('#'))
            .unwrap_or(". --tree --meta")
            .trim_start_matches('$')
            .trim();

        let command = command.strip_prefix("bound ").unwrap_or(command);
        command.split_whitespace().map(ToOwned::to_owned).collect()
    }
}

#[async_trait]
impl RuntimeWorker for ContextBundleWorker {
    async fn init(
        &mut self,
        execution_id: &str,
        grants: Vec<CapabilityGrant>,
        verifier: Arc<CapabilityVerifier>,
        required_capabilities: Vec<Capability>,
        budget: ExecutionBudget,
    ) -> Result<(), String> {
        let caps = verify_task_grants(execution_id, &grants, &verifier, &required_capabilities)?;
        if !caps
            .iter()
            .any(|c| matches!(c, Capability::FilesystemRead { .. }))
        {
            return Err("Context bundling requires FilesystemRead capability".into());
        }

        self.caps = caps;
        self.budget = budget;
        Ok(())
    }

    async fn execute(&self, task: &ExecutionTask) -> Result<WorkerOutput, ExecutionError> {
        if task.source_code.len() > 8_000 {
            return Err(ExecutionError {
                code: "COMMAND_TOO_LARGE".into(),
                message: "Context bundle command exceeds 8KB limit".into(),
                recoverable: false,
                recovery_hint: None,
            });
        }

        let root_dir = self.root_dir()?;
        let bound_bin = tool_binary("BOUND_BIN", "bound", "bound");
        let mut args = Self::bound_args(&task.source_code);
        let output_dir = tempfile::tempdir().map_err(|e| ExecutionError {
            code: "BUNDLE_SETUP_FAILED".into(),
            message: format!("Failed to create bundle output dir: {}", e),
            recoverable: true,
            recovery_hint: Some("Check temp directory permissions".into()),
        })?;
        let output_path = output_dir.path().join("bundle.json");

        args.retain(|arg| arg != "--json");
        args.push("--json".into());
        args.push("--out".into());
        args.push(output_path.to_string_lossy().to_string());

        let wall_clock = self.budget.wall_clock_seconds.max(1);
        let started = std::time::Instant::now();
        let bound_present = executable_present(&bound_bin) || PathBuf::from(&bound_bin).exists();

        let mut command = TokioCommand::new(&bound_bin);
        command
            .args(&args)
            .current_dir(&root_dir)
            .kill_on_drop(true)
            .env_clear()
            .env("PATH", "/usr/local/bin:/usr/bin:/bin");

        let output = match timeout(Duration::from_secs(wall_clock), command.output()).await {
            Ok(Ok(output)) => output,
            Ok(Err(e)) => {
                return Err(ExecutionError {
                    code: "BOUND_SPAWN_FAILED".into(),
                    message: format!("Failed to execute bound: {}", e),
                    recoverable: true,
                    recovery_hint: Some(
                        "Build elci-group/bound and place it on PATH or set BOUND_BIN".into(),
                    ),
                })
            }
            Err(_) => {
                return Err(ExecutionError {
                    code: "TIMEOUT".into(),
                    message: format!("Context bundling exceeded {}s", wall_clock),
                    recoverable: true,
                    recovery_hint: Some("Narrow the filter or increase wall_clock_seconds".into()),
                })
            }
        };

        let duration_ms = started.elapsed().as_millis() as u64;
        let stdout = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr = String::from_utf8_lossy(&output.stderr).to_string();
        let code = output.status.code().unwrap_or(-1);

        if !output.status.success() {
            return Err(ExecutionError {
                code: "BOUND_EXIT_CODE".into(),
                message: format!(
                    "bound exited with code {}. stderr: {}",
                    code,
                    stderr.chars().take(700).collect::<String>()
                ),
                recoverable: true,
                recovery_hint: Some("Inspect bound arguments and readable paths".into()),
            });
        }

        let bundle_text = std::fs::read_to_string(&output_path).map_err(|e| ExecutionError {
            code: "BUNDLE_READ_FAILED".into(),
            message: format!("Failed to read bound JSON output: {}", e),
            recoverable: true,
            recovery_hint: Some("Check bound --out behavior and output permissions".into()),
        })?;
        let bundle_json = serde_json::from_str::<serde_json::Value>(&bundle_text)
            .unwrap_or_else(|_| serde_json::json!({ "raw": bundle_text }));
        let file_count = bundle_json
            .get("files")
            .and_then(|files| files.as_array())
            .map(|files| files.len())
            .unwrap_or(0);

        let mut artifacts = vec![
            Artifact::Json {
                value: bundle_json,
                schema_hash: "bound-json-v1".into(),
            },
            Artifact::Json {
                value: serde_json::json!({
                    "worker": "context_bundle",
                    "executor": "bound",
                    "root_dir": root_dir,
                    "command": args,
                    "file_count": file_count,
                    "duration_ms": duration_ms,
                    "exit_code": code,
                    "bound_present": bound_present
                }),
                schema_hash: "vee-tool-contract-bound-v1".into(),
            },
        ];

        if !stdout.trim().is_empty() {
            artifacts.push(Artifact::Text {
                content: stdout,
                format: TextFormat::Plain,
                line_count: 0,
            });
        }
        if !stderr.trim().is_empty() {
            let mut level_counts = std::collections::HashMap::new();
            level_counts.insert(LogLevel::Warn, stderr.lines().count());
            artifacts.push(Artifact::Log {
                entries: stderr
                    .lines()
                    .map(|message| LogEntry {
                        timestamp: chrono::Utc::now(),
                        level: LogLevel::Warn,
                        message: message.to_string(),
                        source: "bound".into(),
                    })
                    .collect(),
                level_counts,
            });
        }

        Ok(WorkerOutput {
            artifacts,
            stderr,
            exit_code: Some(code),
        })
    }

    async fn shutdown(self: Box<Self>) -> Result<(), String> {
        Ok(())
    }
}
