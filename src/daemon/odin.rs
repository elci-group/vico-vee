use serde_json::json;

use super::ExecutorDaemon;

impl ExecutorDaemon {
    /// Return checkpoint statistics.
    ///
    /// Currently a stub returning zero checkpoints. A persistent checkpoint
    /// recovery layer may be wired in future releases.
    pub async fn checkpoint_stats(&self) -> Result<serde_json::Value, String> {
        Ok(json!({ "checkpoints": 0 }))
    }

    /// Report ODIN health.
    ///
    /// Returns `true` as a placeholder. ODIN is an external ViCo cognitive
    /// orchestrator; real health probing will be added when ODIN is integrated.
    pub async fn odin_health(&self) -> bool {
        true
    }

    /// Return the list of available ODIN models.
    ///
    /// Returns an empty list until the ODIN model registry is integrated.
    pub async fn odin_models(&self) -> Vec<String> {
        vec![]
    }

    /// Set the active ODIN model.
    ///
    /// Currently a no-op placeholder. Model selection will be forwarded to the
    /// ODIN backend once that integration is implemented.
    pub async fn set_odin_model(&self, _model: String) {}
}
