use serde_json::json;

use super::ExecutorDaemon;

impl ExecutorDaemon {
    /// Return checkpoint statistics.
    pub async fn checkpoint_stats(&self) -> Result<serde_json::Value, String> {
        // TODO: wire persistent checkpoint store once recovery layer is integrated.
        Ok(json!({ "checkpoints": 0 }))
    }

    /// Report ODIN health as always-healthy in this stub.
    pub async fn odin_health(&self) -> bool {
        // TODO: implement real ODIN health probe when ODIN service is available.
        true
    }

    /// Return the list of available ODIN models.
    pub async fn odin_models(&self) -> Vec<String> {
        // TODO: query ODIN model registry once it is wired up.
        vec![]
    }

    /// Set the active ODIN model (no-op in this stub).
    pub async fn set_odin_model(&self, _model: String) {
        // TODO: forward model selection to ODIN backend.
    }
}
