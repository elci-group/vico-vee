use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{
    artifact::ArtifactSummary,
    budget::ExecutionBudget,
    result::{ExecutionError, ExecutionPhase},
    schema::{ExecutionHypothesis, ValidationResult},
};

// ─────────────────────────────────────────────────────────────────────────────
// Execution Events
// ─────────────────────────────────────────────────────────────────────────────

/// Structured event emitted during execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "event_type")]
pub enum ExecutionEvent {
    ExecutionStarted {
        execution_id: String,
        task_id: String,
        agent_id: String,
        timestamp: DateTime<Utc>,
        capabilities: Vec<String>,
        budget: ExecutionBudget,
    },
    HypothesisFormed {
        execution_id: String,
        hypothesis: ExecutionHypothesis,
    },
    SimulationCompleted {
        execution_id: String,
        simulation_result: SimulationResult,
    },
    ExecutionProgress {
        execution_id: String,
        phase: ExecutionPhase,
        progress_pct: f64,
        memory_mb: u64,
        cpu_pct: f64,
    },
    ArtifactProduced {
        execution_id: String,
        artifact: ArtifactSummary,
    },
    ValidationCompleted {
        execution_id: String,
        validation: ValidationResult,
    },
    ExecutionCompleted {
        execution_id: String,
        artifacts: Vec<ArtifactSummary>,
        total_duration_ms: u64,
        tokens_consumed: u64,
    },
    ExecutionFailed {
        execution_id: String,
        error: ExecutionError,
        recovery_attempted: bool,
    },
    ResourceWarning {
        execution_id: String,
        resource: String,
        current: f64,
        limit: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResult {
    pub feasible: bool,
    pub estimated_cost: ExecutionBudget,
    pub estimated_confidence: f64,
    pub warnings: Vec<String>,
}
