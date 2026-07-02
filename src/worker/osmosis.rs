//! Osmosis worker implementation.
//!
//! Delegates diff/merge/reject operations for VEE artifacts to the shared
//! `OsmosisEngine`.

use super::core::{verify_task_grants, RuntimeWorker, WorkerOutput};
use crate::artifact::ArtifactStore;
use crate::capability::CapabilityVerifier;
use crate::osmosis::OsmosisEngine;
use crate::types::OsmosisOperation;
use crate::types::*;
use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;

/// Osmosis worker — diff/merge/reject for VEE artifacts.
pub struct OsmosisWorker {
    caps: Vec<Capability>,
    budget: ExecutionBudget,
    engine: OsmosisEngine,
}

impl OsmosisWorker {
    pub fn new(artifact_store: Arc<ArtifactStore>) -> Self {
        Self {
            caps: vec![],
            budget: ExecutionBudget::default(),
            engine: OsmosisEngine::new(artifact_store),
        }
    }

    fn require_write(caps: &[Capability]) -> bool {
        caps.iter().any(|c| {
            matches!(
                c,
                Capability::FilesystemWrite { .. } | Capability::FilesystemCreate { .. }
            )
        })
    }

    fn require_read(caps: &[Capability]) -> bool {
        caps.iter()
            .any(|c| matches!(c, Capability::FilesystemRead { .. }))
    }

    fn parse_project_root(value: &serde_json::Value) -> Option<PathBuf> {
        value
            .get("project_root")
            .and_then(|v| v.as_str())
            .map(PathBuf::from)
    }
}

#[async_trait]
impl RuntimeWorker for OsmosisWorker {
    async fn init(
        &mut self,
        execution_id: &str,
        grants: Vec<CapabilityGrant>,
        verifier: Arc<CapabilityVerifier>,
        required_capabilities: Vec<Capability>,
        budget: ExecutionBudget,
    ) -> Result<(), String> {
        self.caps = verify_task_grants(execution_id, &grants, &verifier, &required_capabilities)?;
        self.budget = budget;
        Ok(())
    }

    async fn execute(&self, task: &ExecutionTask) -> Result<WorkerOutput, ExecutionError> {
        if task.source_code.len() > 100_000 {
            return Err(ExecutionError {
                code: "OSMOSIS_PAYLOAD_TOO_LARGE".into(),
                message: "Osmosis payload exceeds 100KB limit".into(),
                recoverable: false,
                recovery_hint: None,
            });
        }

        let raw: serde_json::Value =
            serde_json::from_str(&task.source_code).map_err(|e| ExecutionError {
                code: "OSMOSIS_PARSE_FAILED".into(),
                message: format!("Failed to parse Osmosis operation: {}", e),
                recoverable: false,
                recovery_hint: Some(
                    "Ensure source_code is a valid OsmosisOperation JSON object".into(),
                ),
            })?;
        let project_root = Self::parse_project_root(&raw);
        let operation: OsmosisOperation =
            serde_json::from_value(raw).map_err(|e| ExecutionError {
                code: "OSMOSIS_PARSE_FAILED".into(),
                message: format!("Failed to parse Osmosis operation: {}", e),
                recoverable: false,
                recovery_hint: Some(
                    "Ensure source_code is a valid OsmosisOperation JSON object".into(),
                ),
            })?;

        match operation {
            OsmosisOperation::Diff(req) => {
                if !Self::require_read(&self.caps) {
                    return Err(ExecutionError {
                        code: "OSMOSIS_CAPABILITY_DENIED".into(),
                        message: "Diff requires a filesystem_read capability".into(),
                        recoverable: false,
                        recovery_hint: Some("Grant filesystem_read to the Osmosis task".into()),
                    });
                }
                let result = self
                    .engine
                    .diff(project_root.as_deref(), &req)
                    .await
                    .map_err(osmosis_error)?;
                let mut artifacts = vec![Artifact::Json {
                    value: serde_json::to_value(&result).unwrap_or_default(),
                    schema_hash: "osmosis-diff-v1".into(),
                }];
                if let Some(unified) = result.unified {
                    artifacts.push(Artifact::Text {
                        content: unified,
                        format: TextFormat::Plain,
                        line_count: 0,
                    });
                }
                Ok(WorkerOutput {
                    artifacts,
                    stderr: String::new(),
                    exit_code: Some(0),
                })
            }
            OsmosisOperation::Merge(req) => {
                if !Self::require_write(&self.caps) {
                    return Err(ExecutionError {
                        code: "OSMOSIS_CAPABILITY_DENIED".into(),
                        message:
                            "Merge requires a filesystem_write or filesystem_create capability"
                                .into(),
                        recoverable: false,
                        recovery_hint: Some("Grant filesystem_write to the Osmosis task".into()),
                    });
                }
                let result = self
                    .engine
                    .merge(project_root.as_deref(), &req)
                    .await
                    .map_err(osmosis_error)?;
                Ok(WorkerOutput {
                    artifacts: vec![
                        Artifact::Json {
                            value: serde_json::to_value(&result).unwrap_or_default(),
                            schema_hash: "osmosis-merge-v1".into(),
                        },
                        Artifact::Text {
                            content: format!(
                                "Merged {} ({} bytes)",
                                result.target_path, result.bytes_written
                            ),
                            format: TextFormat::Plain,
                            line_count: 1,
                        },
                    ],
                    stderr: String::new(),
                    exit_code: Some(0),
                })
            }
            OsmosisOperation::Reject(req) => {
                if !Self::require_write(&self.caps) {
                    return Err(ExecutionError {
                        code: "OSMOSIS_CAPABILITY_DENIED".into(),
                        message:
                            "Reject requires a filesystem_write or filesystem_create capability"
                                .into(),
                        recoverable: false,
                        recovery_hint: Some("Grant filesystem_write to the Osmosis task".into()),
                    });
                }
                let result = self
                    .engine
                    .reject(project_root.as_deref(), &req)
                    .await
                    .map_err(osmosis_error)?;
                Ok(WorkerOutput {
                    artifacts: vec![
                        Artifact::Json {
                            value: serde_json::to_value(&result).unwrap_or_default(),
                            schema_hash: "osmosis-reject-v1".into(),
                        },
                        Artifact::Text {
                            content: format!(
                                "Rejected {} (restored: {})",
                                result.target_path, result.restored
                            ),
                            format: TextFormat::Plain,
                            line_count: 1,
                        },
                    ],
                    stderr: String::new(),
                    exit_code: Some(0),
                })
            }
        }
    }

    async fn shutdown(self: Box<Self>) -> Result<(), String> {
        Ok(())
    }
}

fn osmosis_error(e: String) -> ExecutionError {
    ExecutionError {
        code: "OSMOSIS_FAILED".into(),
        message: e,
        recoverable: true,
        recovery_hint: Some("Check artifact IDs, target paths and capabilities".into()),
    }
}
