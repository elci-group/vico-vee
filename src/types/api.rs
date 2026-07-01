use serde::{Deserialize, Serialize};

use super::budget::ExecutionBudget;

// ─────────────────────────────────────────────────────────────────────────────
// API Request/Response Types
// ─────────────────────────────────────────────────────────────────────────────

/// Request to submit a task for execution.
#[derive(Debug, Deserialize)]
pub struct VeeSubmitRequest {
    pub run_id: Option<String>,
    pub agent_id: String,
    pub language: String,
    pub source_code: String,
    pub capabilities: Vec<String>,
    pub budget: Option<VeeBudgetRequest>,
    pub hypothesis: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct VeeBudgetRequest {
    pub cpu_seconds: Option<u64>,
    pub memory_mb: Option<u64>,
    pub disk_mb: Option<u64>,
    pub token_budget: Option<u64>,
    pub wall_clock_seconds: Option<u64>,
}

impl From<VeeBudgetRequest> for ExecutionBudget {
    fn from(req: VeeBudgetRequest) -> Self {
        Self {
            cpu_seconds: req.cpu_seconds.unwrap_or(30),
            memory_mb: req.memory_mb.unwrap_or(512),
            disk_mb: req.disk_mb.unwrap_or(100),
            token_budget: req.token_budget.unwrap_or(5000),
            wall_clock_seconds: req.wall_clock_seconds.unwrap_or(60),
        }
    }
}

/// Response from a submit request.
#[derive(Debug, Serialize)]
pub struct VeeSubmitResponse {
    pub execution_id: String,
    pub status: String,
    pub estimated_start: String,
}

/// Dashboard statistics.
#[derive(Debug, Serialize)]
pub struct VeeDashboardStats {
    pub total: i64,
    pub completed: i64,
    pub failed: i64,
    pub pending: i64,
    pub avg_latency_ms: i64,
}
