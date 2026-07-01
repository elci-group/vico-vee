use crate::pattern::PatternStore;
use crate::types::TaskSignature;
use serde::{Deserialize, Serialize};

use super::ExecutorDaemon;

/// A minimal pattern record returned by the daemon's pattern lookup.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternRecord {
    pub pattern_id: String,
    pub description: String,
    pub success_rate: f64,
}

impl ExecutorDaemon {
    /// Find patterns matching the given task signature.
    pub async fn find_patterns(&self, signature: &TaskSignature) -> Vec<PatternRecord> {
        let store = PatternStore::new();
        store
            .find_matches(signature, 0.5)
            .into_iter()
            .map(|p| PatternRecord {
                pattern_id: p.pattern_id.clone(),
                description: p.description.clone(),
                success_rate: p.success_rate,
            })
            .collect()
    }
}
