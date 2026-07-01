use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use super::{budget::ExecutionBudget, schema::ExecutionHypothesis, task::ExecutionLanguage};

// ─────────────────────────────────────────────────────────────────────────────
// Execution Patterns (Memory)
// ─────────────────────────────────────────────────────────────────────────────

/// A reusable execution pattern learned from past successes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPattern {
    pub pattern_id: String,
    pub description: String,
    pub task_signature: TaskSignature,
    pub code_template: String,
    pub hypothesis_template: ExecutionHypothesis,
    pub success_rate: f64,
    pub usage_count: u64,
    pub avg_cost: ExecutionBudget,
    pub tags: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSignature {
    pub language: ExecutionLanguage,
    pub intent_keywords: Vec<String>,
    pub required_capabilities: Vec<String>,
    pub estimated_complexity: u8, // 1-10
}
