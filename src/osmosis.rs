//! Osmosis — Patch review lifecycle for VEE artifacts.
//!
//! Osmosis treats VEE outputs as proposed patches. It can diff them against
//! workspace files, merge (apply) them, and reject (revert) them. The engine is
//! intentionally dependency-light: line diffs are computed with a small LCS
//! implementation so Osmosis works everywhere VEE does.

use crate::artifact::ArtifactStore;
use crate::types::*;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// Engine for Osmosis diff / merge / reject operations.
#[derive(Clone)]
pub struct OsmosisEngine {
    artifact_store: Arc<ArtifactStore>,
}

impl OsmosisEngine {
    pub fn new(artifact_store: Arc<ArtifactStore>) -> Self {
        Self { artifact_store }
    }

    /// Diff a VEE artifact against another artifact or a workspace file.
    pub async fn diff(
        &self,
        project_root: Option<&Path>,
        req: &OsmosisDiffRequest,
    ) -> Result<OsmosisDiffResult, String> {
        let left = self
            .resolve_artifact_text(&req.left)
            .await
            .ok_or_else(|| "Could not resolve left artifact as text".to_string())?;

        let (right, right_path) = if let Some(right_ref) = &req.right {
            let text = self
                .resolve_artifact_text(right_ref)
                .await
                .ok_or_else(|| "Could not resolve right artifact as text".to_string())?;
            (text, Some(right_ref.execution_id.clone()))
        } else if let Some(target) = &req.target_path {
            let path = Self::resolve_workspace_path(project_root, target)?;
            let text = tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| format!("Failed to read {}: {}", path.display(), e))?;
            (text, Some(target.clone()))
        } else {
            return Err("Osmosis diff requires either right artifact or target_path".to_string());
        };

        let structured = diff_text(&left, &right);
        let unified = if req.format.as_ref() == Some(&OsmosisDiffFormat::Unified) {
            Some(structured_to_unified(&structured))
        } else {
            None
        };

        Ok(OsmosisDiffResult {
            left_path: Some(req.left.execution_id.clone()),
            right_path,
            structured,
            unified,
        })
    }

    /// Apply a VEE artifact to a workspace file.
    pub async fn merge(
        &self,
        project_root: Option<&Path>,
        req: &OsmosisMergeRequest,
    ) -> Result<OsmosisMergeResult, String> {
        let source = self
            .resolve_artifact_text(&req.source)
            .await
            .ok_or_else(|| "Could not resolve source artifact as text".to_string())?;

        let path = Self::resolve_workspace_path(project_root, &req.target_path)?;
        let strategy = req.strategy.clone().unwrap_or_default();

        let content = match strategy {
            OsmosisMergeStrategy::Overwrite => source,
            OsmosisMergeStrategy::Append => {
                let existing = tokio::fs::read_to_string(&path).await.unwrap_or_default();
                format!("{}\n{}", existing, source)
            }
        };

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent).await.map_err(|e| {
                format!("Failed to create parent dirs for {}: {}", path.display(), e)
            })?;
        }
        tokio::fs::write(&path, &content)
            .await
            .map_err(|e| format!("Failed to write {}: {}", path.display(), e))?;

        Ok(OsmosisMergeResult {
            target_path: req.target_path.clone(),
            bytes_written: content.len(),
            strategy: match strategy {
                OsmosisMergeStrategy::Overwrite => "overwrite".to_string(),
                OsmosisMergeStrategy::Append => "append".to_string(),
            },
        })
    }

    /// Reject a proposed patch and restore a base version if one is supplied.
    pub async fn reject(
        &self,
        project_root: Option<&Path>,
        req: &OsmosisRejectRequest,
    ) -> Result<OsmosisRejectResult, String> {
        let path = Self::resolve_workspace_path(project_root, &req.target_path)?;

        let restored = if let Some(base_ref) = &req.base {
            let base = self
                .resolve_artifact_text(base_ref)
                .await
                .ok_or_else(|| "Could not resolve base artifact as text".to_string())?;
            if let Some(parent) = path.parent() {
                tokio::fs::create_dir_all(parent).await.map_err(|e| {
                    format!("Failed to create parent dirs for {}: {}", path.display(), e)
                })?;
            }
            tokio::fs::write(&path, base)
                .await
                .map_err(|e| format!("Failed to restore {}: {}", path.display(), e))?;
            true
        } else {
            // If no base is supplied and the file currently matches the rejected
            // patch, assume the patch created it and remove it.
            if let Ok(current) = tokio::fs::read_to_string(&path).await {
                if let Some(source) = self.resolve_artifact_text(&req.source).await {
                    if current == source {
                        tokio::fs::remove_file(&path)
                            .await
                            .map_err(|e| format!("Failed to remove {}: {}", path.display(), e))?;
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            } else {
                false
            }
        };

        Ok(OsmosisRejectResult {
            target_path: req.target_path.clone(),
            restored,
            reason: req.reason.clone(),
        })
    }

    /// Resolve an artifact reference to UTF-8 text.
    ///
    /// Supports `Text`, `Json` and `File` artifacts. If `artifact_id` is `None`,
    /// the first text-like artifact from the execution is used.
    async fn resolve_artifact_text(&self, reference: &OsmosisArtifactRef) -> Option<String> {
        let artifact = if let Some(id) = &reference.artifact_id {
            self.artifact_store.get(id).await
        } else {
            self.artifact_store
                .get_by_execution(&reference.execution_id)
                .await
                .into_iter()
                .find_map(|(_id, art)| if text_artifact(&art) { Some(art) } else { None })
        }?;

        match &artifact {
            Artifact::Text { content, .. } => Some(content.clone()),
            Artifact::Json { value, .. } => Some(value.to_string()),
            Artifact::File { path, .. } => tokio::fs::read_to_string(path).await.ok(),
            _ => None,
        }
    }

    /// Resolve a workspace-relative path and ensure it stays within the project root.
    fn resolve_workspace_path(
        project_root: Option<&Path>,
        target: &str,
    ) -> Result<PathBuf, String> {
        let root = project_root.ok_or_else(|| "No project workspace is open".to_string())?;
        let full = normalize_path(root, target);
        if !full.starts_with(root) {
            return Err(format!("Target path escapes project root: {}", target));
        }
        Ok(full)
    }
}

/// Resolve `target` relative to `root`, normalising `.`, `..` and repeated separators
/// without requiring the path to exist.
fn normalize_path(root: &Path, target: &str) -> PathBuf {
    let mut stack = Vec::new();
    for component in Path::new(target).components() {
        match component {
            std::path::Component::Prefix(_) | std::path::Component::RootDir => {
                // Ignore absolute prefixes; the path is resolved relative to root.
            }
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if let Some(last) = stack.last() {
                    if last != &std::path::Component::ParentDir {
                        stack.pop();
                        continue;
                    }
                }
                stack.push(std::path::Component::ParentDir);
            }
            std::path::Component::Normal(name) => stack.push(std::path::Component::Normal(name)),
        }
    }
    let relative: PathBuf = stack.into_iter().collect();
    root.join(relative)
}

fn text_artifact(artifact: &Artifact) -> bool {
    matches!(
        artifact,
        Artifact::Text { .. } | Artifact::Json { .. } | Artifact::File { .. }
    )
}

// ─────────────────────────────────────────────────────────────────────────────
// Line-level text diff
// ─────────────────────────────────────────────────────────────────────────────

const HUNK_CONTEXT: usize = 3;

/// Compute a structured line-level diff between two text blobs.
pub fn diff_text(old_text: &str, new_text: &str) -> OsmosisStructuredDiff {
    let old_lines: Vec<&str> = old_text.lines().collect();
    let new_lines: Vec<&str> = new_text.lines().collect();

    let lcs = lcs_indices(&old_lines, &new_lines);
    let ops = build_ops(&old_lines, &new_lines, &lcs);

    let file_diff = if ops.is_empty() || ops.iter().all(|o| matches!(o, DiffOp::Context(_))) {
        OsmosisFileDiff {
            old_path: None,
            new_path: None,
            status: "Unchanged".to_string(),
            hunks: vec![],
        }
    } else {
        build_file_diff(&ops)
    };

    OsmosisStructuredDiff {
        files: vec![file_diff],
    }
}

#[derive(Debug, Clone)]
enum DiffOp<'a> {
    Context(&'a str),
    Remove(&'a str),
    Add(&'a str),
}

fn lcs_indices<T: Eq>(a: &[T], b: &[T]) -> Vec<(usize, usize)> {
    if a.is_empty() || b.is_empty() {
        return vec![];
    }

    // Dynamic programming with two rows to keep memory O(min(n,m)).
    let (a, b, swapped) = if a.len() < b.len() {
        (a, b, false)
    } else {
        (b, a, true)
    };

    let n = a.len();
    let m = b.len();
    let mut prev = vec![0usize; m + 1];
    let mut curr = vec![0usize; m + 1];
    let mut choice = vec![vec![0u8; m + 1]; n + 1];

    for i in 1..=n {
        for j in 1..=m {
            if a[i - 1] == b[j - 1] {
                curr[j] = prev[j - 1] + 1;
                choice[i][j] = 1; // diagonal
            } else if prev[j] >= curr[j - 1] {
                curr[j] = prev[j];
                choice[i][j] = 2; // up
            } else {
                curr[j] = curr[j - 1];
                choice[i][j] = 3; // left
            }
        }
        std::mem::swap(&mut prev, &mut curr);
    }

    // Reconstruct the LCS in reverse.
    let mut i = n;
    let mut j = m;
    let mut rev = Vec::new();
    while i > 0 && j > 0 {
        match choice[i][j] {
            1 => {
                rev.push((i - 1, j - 1));
                i -= 1;
                j -= 1;
            }
            2 => i -= 1,
            3 => j -= 1,
            _ => unreachable!(),
        }
    }
    rev.reverse();

    if swapped {
        rev.into_iter().map(|(x, y)| (y, x)).collect()
    } else {
        rev
    }
}

fn build_ops<'a>(old: &[&'a str], new: &[&'a str], lcs: &[(usize, usize)]) -> Vec<DiffOp<'a>> {
    let mut ops = Vec::new();
    let mut oi = 0usize;
    let mut ni = 0usize;
    for &(omi, nmi) in lcs {
        while oi < omi {
            ops.push(DiffOp::Remove(old[oi]));
            oi += 1;
        }
        while ni < nmi {
            ops.push(DiffOp::Add(new[ni]));
            ni += 1;
        }
        ops.push(DiffOp::Context(old[oi]));
        oi += 1;
        ni += 1;
    }
    while oi < old.len() {
        ops.push(DiffOp::Remove(old[oi]));
        oi += 1;
    }
    while ni < new.len() {
        ops.push(DiffOp::Add(new[ni]));
        ni += 1;
    }
    ops
}

fn build_file_diff(ops: &[DiffOp<'_>]) -> OsmosisFileDiff {
    let first_change = ops
        .iter()
        .position(|o| !matches!(o, DiffOp::Context(_)))
        .unwrap_or(0);
    let last_change = ops
        .iter()
        .rposition(|o| !matches!(o, DiffOp::Context(_)))
        .unwrap_or(ops.len() - 1);

    let start = first_change.saturating_sub(HUNK_CONTEXT);
    let end = (last_change + HUNK_CONTEXT + 1).min(ops.len());
    let hunk_ops = &ops[start..end];

    let old_start = count_old_lines(&ops[..start]) + 1;
    let new_start = count_new_lines(&ops[..start]) + 1;
    let old_lines = count_old_lines(hunk_ops);
    let new_lines = count_new_lines(hunk_ops);

    let mut lines = Vec::new();
    let mut old_line = old_start;
    let mut new_line = new_start;
    for op in hunk_ops {
        match op {
            DiffOp::Context(s) => {
                lines.push(OsmosisDiffLine {
                    kind: "context".to_string(),
                    content: s.to_string(),
                    old_line: Some(old_line),
                    new_line: Some(new_line),
                });
                old_line += 1;
                new_line += 1;
            }
            DiffOp::Remove(s) => {
                lines.push(OsmosisDiffLine {
                    kind: "remove".to_string(),
                    content: s.to_string(),
                    old_line: Some(old_line),
                    new_line: None,
                });
                old_line += 1;
            }
            DiffOp::Add(s) => {
                lines.push(OsmosisDiffLine {
                    kind: "add".to_string(),
                    content: s.to_string(),
                    old_line: None,
                    new_line: Some(new_line),
                });
                new_line += 1;
            }
        }
    }

    OsmosisFileDiff {
        old_path: None,
        new_path: None,
        status: "Modified".to_string(),
        hunks: vec![OsmosisHunk {
            old_start,
            old_lines,
            new_start,
            new_lines,
            lines,
        }],
    }
}

fn count_old_lines(ops: &[DiffOp<'_>]) -> usize {
    ops.iter()
        .filter(|o| matches!(o, DiffOp::Context(_) | DiffOp::Remove(_)))
        .count()
}

fn count_new_lines(ops: &[DiffOp<'_>]) -> usize {
    ops.iter()
        .filter(|o| matches!(o, DiffOp::Context(_) | DiffOp::Add(_)))
        .count()
}

/// Convert a structured diff into a unified diff string.
pub fn structured_to_unified(diff: &OsmosisStructuredDiff) -> String {
    let mut out = String::new();
    for file in &diff.files {
        let old_path = file.old_path.as_deref().unwrap_or("a/file");
        let new_path = file.new_path.as_deref().unwrap_or("b/file");
        out.push_str(&format!("--- {}\n", old_path));
        out.push_str(&format!("+++ {}\n", new_path));
        for hunk in &file.hunks {
            out.push_str(&format!(
                "@@ -{},{} +{},{} @@\n",
                hunk.old_start, hunk.old_lines, hunk.new_start, hunk.new_lines
            ));
            for line in &hunk.lines {
                let prefix = match line.kind.as_str() {
                    "add" => "+",
                    "remove" => "-",
                    _ => " ",
                };
                out.push_str(prefix);
                out.push_str(&line.content);
                out.push('\n');
            }
        }
    }
    out
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{Artifact, ExecutionBudget, ExecutionTask, TextFormat};

    #[test]
    fn diff_detects_single_line_change() {
        let diff = diff_text("hello\nworld\n", "hello\nViCo\n");
        assert_eq!(diff.files.len(), 1);
        let file = &diff.files[0];
        assert_eq!(file.status, "Modified");
        assert_eq!(file.hunks.len(), 1);
        let kinds: Vec<&str> = file.hunks[0]
            .lines
            .iter()
            .map(|l| l.kind.as_str())
            .collect();
        assert!(kinds.contains(&"remove"));
        assert!(kinds.contains(&"add"));
    }

    #[test]
    fn diff_reports_unchanged() {
        let diff = diff_text("same\n", "same\n");
        assert_eq!(diff.files[0].status, "Unchanged");
        assert!(diff.files[0].hunks.is_empty());
    }

    #[test]
    fn unified_format_includes_headers() {
        let diff = diff_text("a\nb\n", "a\nc\n");
        let unified = structured_to_unified(&diff);
        assert!(unified.contains("--- "));
        assert!(unified.contains("+++ "));
        assert!(unified.contains("@@"));
        assert!(unified.contains("-b"));
        assert!(unified.contains("+c"));
    }

    #[tokio::test]
    async fn osmosis_worker_executes_diff() {
        let tmp = tempfile::tempdir().unwrap();
        let store = Arc::new(
            crate::artifact::ArtifactStore::try_new(
                &tmp.path().join("vee_artifacts.db"),
                &tmp.path().join("artifacts"),
            )
            .unwrap(),
        );

        let execution_id = "exec-osmosis-worker";
        let provenance = crate::types::Provenance {
            artifact_id: "art-left".into(),
            task_id: execution_id.into(),
            execution_id: execution_id.into(),
            creator_agent: "test".into(),
            parent_artifacts: vec![],
            code_generator: "test".into(),
            executed_code: "old".into(),
            granted_capabilities: vec![],
            created_at: chrono::Utc::now(),
            previous_hash: "genesis".into(),
            self_hash: String::new(),
        };

        let left = Artifact::Text {
            content: "alpha\nbeta\n".into(),
            format: TextFormat::Plain,
            line_count: 2,
        };
        let left_id = store.store(left, Some(provenance.clone())).await.unwrap();

        let right = Artifact::Text {
            content: "alpha\ngamma\n".into(),
            format: TextFormat::Plain,
            line_count: 2,
        };
        let mut right_prov = provenance.clone();
        right_prov.executed_code = "new".into();
        let right_id = store.store(right, Some(right_prov)).await.unwrap();

        let operation = OsmosisOperation::Diff(OsmosisDiffRequest {
            left: OsmosisArtifactRef {
                execution_id: execution_id.into(),
                artifact_id: Some(left_id),
            },
            right: Some(OsmosisArtifactRef {
                execution_id: execution_id.into(),
                artifact_id: Some(right_id),
            }),
            target_path: None,
            format: Some(OsmosisDiffFormat::Structured),
        });

        let task = ExecutionTask {
            execution_id: execution_id.into(),
            run_id: None,
            agent_id: "test".into(),
            language: crate::types::ExecutionLanguage::Osmosis,
            source_code: serde_json::to_string(&operation).unwrap(),
            capabilities: vec![crate::types::Capability::FilesystemRead {
                paths: vec!["*".into()],
            }],
            capability_grants: vec![],
            project_id: None,
            budget: ExecutionBudget {
                cpu_seconds: 1,
                memory_mb: 64,
                disk_mb: 10,
                token_budget: 0,
                wall_clock_seconds: 5,
            },
            hypothesis: None,
            provenance: provenance.clone(),
        };

        let mut registry = crate::capability::CapabilityRegistry::new_with_seed([33u8; 32]);
        let grant = registry.grant(
            execution_id,
            crate::types::Capability::FilesystemRead {
                paths: vec!["*".into()],
            },
            crate::types::GrantAuthority::Orchestrator,
            None,
        );
        let verifier = Arc::new(registry.verifier());

        let mut worker =
            crate::worker::create_worker(crate::types::ExecutionLanguage::Osmosis, store)
                .expect("osmosis worker should be creatable");
        worker
            .init(
                execution_id,
                vec![grant],
                verifier,
                task.capabilities.clone(),
                task.budget.clone(),
            )
            .await
            .unwrap();
        let output = worker.execute(&task).await.unwrap();

        let has_diff_json = output.artifacts.iter().any(|a| matches!(a, Artifact::Json { value, schema_hash } if schema_hash == "osmosis-diff-v1" && value.get("structured").is_some()));
        assert!(has_diff_json);
    }
}
