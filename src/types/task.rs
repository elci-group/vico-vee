use serde::{Deserialize, Serialize};

use super::{
    budget::ExecutionBudget,
    capability::{Capability, CapabilityGrant},
    provenance::Provenance,
    schema::ExecutionHypothesis,
};

// ─────────────────────────────────────────────────────────────────────────────
// Execution Task & Result
// ─────────────────────────────────────────────────────────────────────────────

/// Supported execution languages.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ExecutionLanguage {
    Python,
    Rust,
    JavaScript,
    Go,
    ContextBundle,
    Shell,
    Wasm,
    Osmosis,
}

impl std::fmt::Display for ExecutionLanguage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExecutionLanguage::Python => write!(f, "python"),
            ExecutionLanguage::Rust => write!(f, "rust"),
            ExecutionLanguage::JavaScript => write!(f, "javascript"),
            ExecutionLanguage::Go => write!(f, "go"),
            ExecutionLanguage::ContextBundle => write!(f, "context_bundle"),
            ExecutionLanguage::Shell => write!(f, "shell"),
            ExecutionLanguage::Wasm => write!(f, "wasm"),
            ExecutionLanguage::Osmosis => write!(f, "osmosis"),
        }
    }
}

/// A task submitted for execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionTask {
    pub execution_id: String,
    pub run_id: Option<String>,
    pub agent_id: String,
    pub language: ExecutionLanguage,
    pub source_code: String,
    pub capabilities: Vec<Capability>,
    /// Signed capability grants issued by the orchestrator/authority. The
    /// executor daemon and each worker verify these before exercising a
    /// capability.
    #[serde(default)]
    pub capability_grants: Vec<CapabilityGrant>,
    pub budget: ExecutionBudget,
    pub hypothesis: Option<ExecutionHypothesis>,
    pub provenance: Provenance,
    /// Project that owns this task. When `None` the daemon falls back to the
    /// `default` project.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub project_id: Option<String>,
}
