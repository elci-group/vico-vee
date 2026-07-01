use serde::{Deserialize, Serialize};

// ─────────────────────────────────────────────────────────────────────────────
// Osmosis — Patch Review Lifecycle for VEE Artifacts
// ─────────────────────────────────────────────────────────────────────────────

/// Reference to a stored VEE artifact.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsmosisArtifactRef {
    pub execution_id: String,
    pub artifact_id: Option<String>,
}

/// Output format for an Osmosis diff.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OsmosisDiffFormat {
    #[default]
    Structured,
    Unified,
}

/// Merge strategy when applying an artifact to a workspace file.
#[derive(Debug, Default, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum OsmosisMergeStrategy {
    #[default]
    Overwrite,
    Append,
}

/// Diff a VEE artifact against another artifact or a workspace file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsmosisDiffRequest {
    pub left: OsmosisArtifactRef,
    pub right: Option<OsmosisArtifactRef>,
    pub target_path: Option<String>,
    pub format: Option<OsmosisDiffFormat>,
}

/// Apply a VEE artifact to a workspace file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsmosisMergeRequest {
    pub source: OsmosisArtifactRef,
    pub target_path: String,
    pub strategy: Option<OsmosisMergeStrategy>,
    pub base: Option<OsmosisArtifactRef>,
}

/// Reject a VEE artifact patch and optionally restore a base version.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsmosisRejectRequest {
    pub source: OsmosisArtifactRef,
    pub target_path: String,
    pub base: Option<OsmosisArtifactRef>,
    pub reason: Option<String>,
}

/// A line-level structured diff result compatible with the frontend diff viewer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsmosisStructuredDiff {
    pub files: Vec<OsmosisFileDiff>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsmosisFileDiff {
    pub old_path: Option<String>,
    pub new_path: Option<String>,
    pub status: String,
    pub hunks: Vec<OsmosisHunk>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsmosisHunk {
    pub old_start: usize,
    pub old_lines: usize,
    pub new_start: usize,
    pub new_lines: usize,
    pub lines: Vec<OsmosisDiffLine>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsmosisDiffLine {
    pub kind: String,
    pub content: String,
    pub old_line: Option<usize>,
    pub new_line: Option<usize>,
}

/// Result of an Osmosis diff operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsmosisDiffResult {
    pub left_path: Option<String>,
    pub right_path: Option<String>,
    pub structured: OsmosisStructuredDiff,
    pub unified: Option<String>,
}

/// Result of an Osmosis merge operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsmosisMergeResult {
    pub target_path: String,
    pub bytes_written: usize,
    pub strategy: String,
}

/// Result of an Osmosis reject operation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OsmosisRejectResult {
    pub target_path: String,
    pub restored: bool,
    pub reason: Option<String>,
}

/// Osmosis operation wrapper used by the Osmosis worker.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", content = "spec")]
pub enum OsmosisOperation {
    Diff(OsmosisDiffRequest),
    Merge(OsmosisMergeRequest),
    Reject(OsmosisRejectRequest),
}
