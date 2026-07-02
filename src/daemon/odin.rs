//! ODIN integration backed by a local Ollama instance.

use serde::Deserialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Client for the Ollama HTTP API used by ODIN.
#[derive(Clone)]
pub struct OdinClient {
    client: reqwest::Client,
    base_url: String,
}

impl OdinClient {
    /// Create a new client pointing at the given Ollama base URL.
    pub fn new(base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
        }
    }

    /// Probe whether Ollama is reachable.
    pub async fn health(&self) -> bool {
        match self.client.get(format!("{}/", self.base_url)).send().await {
            Ok(resp) => resp.status().is_success(),
            Err(e) => {
                tracing::debug!(error = %e, "ODIN health probe failed");
                false
            }
        }
    }

    /// List available model names from `/api/tags`.
    pub async fn models(&self) -> Vec<String> {
        match self
            .client
            .get(format!("{}/api/tags", self.base_url))
            .send()
            .await
        {
            Ok(resp) if resp.status().is_success() => match resp.json::<TagsResponse>().await {
                Ok(tags) => tags.models.into_iter().map(|m| m.name).collect(),
                Err(e) => {
                    tracing::warn!(error = %e, "failed to parse Ollama tags response");
                    vec![]
                }
            },
            Ok(resp) => {
                tracing::warn!(status = %resp.status(), "Ollama tags request failed");
                vec![]
            }
            Err(e) => {
                tracing::debug!(error = %e, "Ollama tags request failed");
                vec![]
            }
        }
    }
}

#[derive(Debug, Deserialize)]
struct TagsResponse {
    models: Vec<ModelTag>,
}

#[derive(Debug, Deserialize)]
struct ModelTag {
    name: String,
}

/// Runtime ODIN state held by the daemon.
#[derive(Clone, Default)]
pub struct OdinState {
    pub client: Option<Arc<OdinClient>>,
    pub active_model: Arc<Mutex<String>>,
    pub reachable: Arc<AtomicBool>,
}

impl OdinState {
    pub fn new(client: Option<OdinClient>) -> Self {
        Self {
            client: client.map(Arc::new),
            active_model: Arc::new(Mutex::new(String::new())),
            reachable: Arc::new(AtomicBool::new(false)),
        }
    }
}

use super::ExecutorDaemon;

impl ExecutorDaemon {
    /// Return checkpoint statistics.
    pub async fn checkpoint_stats(&self) -> Result<serde_json::Value, String> {
        Ok(serde_json::json!({ "checkpoints": 0 }))
    }

    /// Report ODIN / Ollama health.
    pub async fn odin_health(&self) -> bool {
        if let Some(client) = &self.inner.odin_state.client {
            let healthy = client.health().await;
            self.inner
                .odin_state
                .reachable
                .store(healthy, Ordering::Relaxed);
            healthy
        } else {
            false
        }
    }

    /// Return the list of available ODIN models.
    pub async fn odin_models(&self) -> Vec<String> {
        if let Some(client) = &self.inner.odin_state.client {
            client.models().await
        } else {
            vec![]
        }
    }

    /// Return the currently selected ODIN model.
    pub async fn odin_active_model(&self) -> String {
        self.inner
            .odin_state
            .active_model
            .lock()
            .map(|g| g.clone())
            .unwrap_or_default()
    }

    /// Set the active ODIN model.
    pub async fn set_odin_model(&self, model: String) {
        if let Ok(mut guard) = self.inner.odin_state.active_model.lock() {
            *guard = model;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn odin_client_health_detects_unreachable() {
        let client = OdinClient::new("http://127.0.0.1:1".to_string());
        assert!(!client.health().await);
    }

    #[tokio::test]
    async fn odin_models_returns_empty_when_unreachable() {
        let client = OdinClient::new("http://127.0.0.1:1".to_string());
        assert!(client.models().await.is_empty());
    }
}
