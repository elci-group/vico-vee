use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Provenance
// ─────────────────────────────────────────────────────────────────────────────

/// Full lineage for every artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Provenance {
    pub artifact_id: String,
    pub task_id: String,
    pub execution_id: String,
    pub creator_agent: String,
    pub parent_artifacts: Vec<String>,
    pub code_generator: String,
    pub executed_code: String,
    pub granted_capabilities: Vec<String>,
    pub created_at: DateTime<Utc>,
    pub previous_hash: String,
    pub self_hash: String,
    /// Project that owns this artifact. When `None` the artifact falls back to
    /// the `default` project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}

impl Default for Provenance {
    fn default() -> Self {
        Self {
            artifact_id: String::new(),
            task_id: String::new(),
            execution_id: String::new(),
            creator_agent: String::new(),
            parent_artifacts: vec![],
            code_generator: String::new(),
            executed_code: String::new(),
            granted_capabilities: vec![],
            created_at: Utc::now(),
            previous_hash: String::new(),
            self_hash: String::new(),
            project_id: None,
        }
    }
}
