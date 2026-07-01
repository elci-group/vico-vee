use crate::types::{Capability, ExecutionBudget};
use std::path::PathBuf;

/// Configuration for a single sandboxed execution.
pub struct SandboxConfig {
    /// Working directory (read-write scratch space)
    pub work_dir: PathBuf,
    /// Output directory (where artifacts are written)
    pub output_dir: PathBuf,
    /// Read-only input paths
    pub input_paths: Vec<PathBuf>,
    /// Paths that may be executed (e.g., system binary directories)
    pub executable_paths: Vec<PathBuf>,
    /// Resource budget
    pub budget: ExecutionBudget,
    /// Granted capabilities
    pub capabilities: Vec<Capability>,
    /// Whether network should be blocked
    pub block_network: bool,
}

/// Result of a sandboxed execution.
pub struct SandboxResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub memory_peak_kb: u64,
    pub sandbox_layers_applied: Vec<String>,
    pub sandbox_errors: Vec<String>,
}
