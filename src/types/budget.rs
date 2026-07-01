use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Execution Budget
// ─────────────────────────────────────────────────────────────────────────────

/// Resource budget for a single execution. Enforced, not suggested.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ExecutionBudget {
    /// CPU time limit (seconds)
    pub cpu_seconds: u64,
    /// Memory limit (megabytes)
    pub memory_mb: u64,
    /// Disk limit (megabytes)
    pub disk_mb: u64,
    /// Token budget for LLM calls during execution
    pub token_budget: u64,
    /// Maximum wall-clock time (seconds)
    pub wall_clock_seconds: u64,
}
