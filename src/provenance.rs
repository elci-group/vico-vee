//! Provenance Tracking
//!
//! Git-style hash chain for tamper-evident artifact lineage.

use crate::types::{Artifact, Provenance};
use sha2::{Digest, Sha256};

/// Build a provenance record for a new artifact.
#[allow(clippy::too_many_arguments)]
pub fn build_provenance(
    artifact_id: &str,
    execution_id: &str,
    task_id: &str,
    creator_agent: &str,
    parent_artifacts: Vec<String>,
    code_generator: &str,
    executed_code: &str,
    granted_capabilities: Vec<String>,
    previous_hash: &str,
) -> Provenance {
    let created_at = chrono::Utc::now();
    let self_hash = compute_hash(
        artifact_id,
        execution_id,
        task_id,
        creator_agent,
        &parent_artifacts,
        code_generator,
        executed_code,
        &granted_capabilities,
        previous_hash,
        &created_at.to_rfc3339(),
    );

    Provenance {
        artifact_id: artifact_id.to_string(),
        task_id: task_id.to_string(),
        execution_id: execution_id.to_string(),
        project_id: None,
        creator_agent: creator_agent.to_string(),
        parent_artifacts,
        code_generator: code_generator.to_string(),
        executed_code: executed_code.to_string(),
        granted_capabilities,
        created_at,
        previous_hash: previous_hash.to_string(),
        self_hash,
    }
}

/// Verify that a provenance record's hash is correct.
pub fn verify_provenance(provenance: &Provenance) -> bool {
    let expected = compute_hash(
        &provenance.artifact_id,
        &provenance.execution_id,
        &provenance.task_id,
        &provenance.creator_agent,
        &provenance.parent_artifacts,
        &provenance.code_generator,
        &provenance.executed_code,
        &provenance.granted_capabilities,
        &provenance.previous_hash,
        &provenance.created_at.to_rfc3339(),
    );
    expected == provenance.self_hash
}

#[allow(clippy::too_many_arguments)]
fn compute_hash(
    artifact_id: &str,
    execution_id: &str,
    task_id: &str,
    creator_agent: &str,
    parent_artifacts: &[String],
    code_generator: &str,
    executed_code: &str,
    granted_capabilities: &[String],
    previous_hash: &str,
    timestamp: &str,
) -> String {
    let input = format!(
        "{}:{}:{}:{}:{}:{}:{}:{}:{}:{}",
        artifact_id,
        execution_id,
        task_id,
        creator_agent,
        parent_artifacts.join(","),
        code_generator,
        executed_code,
        granted_capabilities.join(","),
        previous_hash,
        timestamp,
    );
    let hash = Sha256::digest(input.as_bytes());
    format!("{:x}", hash)
}

/// Extract provenance from an artifact.
/// Currently returns None until Artifact stores provenance natively.
pub fn artifact_provenance(_artifact: &Artifact) -> Option<&Provenance> {
    None
}
