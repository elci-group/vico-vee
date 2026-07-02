use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::artifact::Artifact;
use super::schema::ValidationResult;

/// Result of executing a task.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    pub execution_id: String,
    pub project_id: Option<String>,
    pub status: ExecutionStatus,
    pub phase: ExecutionPhase,
    pub artifacts: Vec<Artifact>,
    pub validation: Option<ValidationResult>,
    pub confidence: f64,
    pub tokens_consumed: u64,
    pub cpu_seconds_used: f64,
    pub memory_peak_mb: f64,
    pub latency_ms: u64,
    pub error_log: Option<String>,
    pub created_at: DateTime<Utc>,
    pub started_at: Option<DateTime<Utc>>,
    pub completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ExecutionStatus {
    Pending,
    Queued,
    Simulating,
    Executing,
    Validating,
    Completed,
    Failed,
    Recovered,
    Cancelled,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ExecutionPhase {
    Hypothesis,
    Simulation,
    Execution,
    Validation,
    Feedback,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionError {
    pub code: String,
    pub message: String,
    pub recoverable: bool,
    pub recovery_hint: Option<String>,
}
