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
        if let Some(store) = &self.inner.pattern_store {
            let store = store.clone();
            let signature = signature.clone();
            let patterns = tokio::task::spawn_blocking(move || {
                store.find_matches(&signature, 0.5)
            })
            .await
            .unwrap_or_default();
            patterns
                .into_iter()
                .map(|p| PatternRecord {
                    pattern_id: p.pattern_id,
                    description: p.description,
                    success_rate: p.success_rate,
                })
                .collect()
        } else {
            vec![]
        }
    }
}
