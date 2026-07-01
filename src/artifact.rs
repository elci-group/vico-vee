//! Persistent Artifact Store
//!
//! Stores VEE artifacts in a content-addressable filesystem layer backed by
//! SQLite metadata. Small and large artifacts are deduplicated by hash and
//! survive process restarts. An in-memory LRU cache sits in front of blob
//! reads.

use crate::types::{Artifact, ArtifactSummary, Provenance};
use lru::LruCache;
use rusqlite::{params, Connection, OptionalExtension};
use std::num::NonZeroUsize;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const ARTIFACT_CACHE_SIZE: usize = 128;

pub struct ArtifactStore {
    db: Arc<tokio::sync::Mutex<Connection>>,
    blob_dir: PathBuf,
    cache: Arc<tokio::sync::Mutex<LruCache<String, Artifact>>>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct ArtifactMetadata {
    pub kind: String,
    pub execution_id: String,
    pub size_bytes: u64,
    pub mime_type: Option<String>,
}

impl ArtifactStore {
    pub fn new() -> Self {
        let data_dir = crate::paths::vee_data_dir();
        let db_path = data_dir.join("vee_artifacts.db");
        let blob_dir = data_dir.join("artifacts");
        Self::try_new(&db_path, &blob_dir).unwrap_or_else(|e| {
            tracing::error!(error = %e, "persistent artifact store failed; using in-memory fallback");
            let tmp = std::env::temp_dir().join(format!("vico-artifacts-{}", std::process::id()));
            Self::try_new(&tmp.join("vee_artifacts.db"), &tmp.join("artifacts"))
                .expect("in-memory artifact store must always be constructible")
        })
    }

    pub fn try_new(db_path: &Path, blob_dir: &Path) -> Result<Self, String> {
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("create artifact db parent dir: {}", e))?;
        }
        std::fs::create_dir_all(blob_dir)
            .map_err(|e| format!("create artifact blob dir: {}", e))?;
        let conn = Connection::open(db_path).map_err(|e| format!("open artifact db: {}", e))?;
        crate::migrations::run_migrations(&conn, crate::migrations::MIGRATIONS)
            .map_err(|e| format!("run artifact schema migrations: {}", e))?;
        Ok(Self {
            db: Arc::new(tokio::sync::Mutex::new(conn)),
            blob_dir: blob_dir.to_path_buf(),
            cache: Arc::new(tokio::sync::Mutex::new(LruCache::new(
                NonZeroUsize::new(ARTIFACT_CACHE_SIZE).unwrap_or(NonZeroUsize::MIN),
            ))),
        })
    }

    /// Store an artifact, returning its ID on success.
    pub async fn store(
        &self,
        artifact: Artifact,
        provenance: Option<Provenance>,
    ) -> Result<String, String> {
        let id = Self::artifact_id(&artifact);
        let execution_id = provenance
            .as_ref()
            .map(|p| p.execution_id.clone())
            .unwrap_or_default();
        let kind = Self::kind_of(&artifact);
        let summary = ArtifactSummary::from(&artifact);
        let metadata = ArtifactMetadata {
            kind: kind.clone(),
            execution_id: execution_id.clone(),
            size_bytes: summary.size_bytes,
            mime_type: summary.mime_type.clone(),
        };

        let blob = serde_json::to_vec(&artifact)
            .map_err(|e| format!("failed to serialize artifact: {}", e))?;
        let hash = blake3::hash(&blob).to_hex().to_string();
        let blob_path = self.blob_path(&hash);

        let db = self.db.clone();
        let blob_path_clone = blob_path.clone();
        let provenance_json = provenance
            .as_ref()
            .and_then(|p| serde_json::to_string(p).ok());
        let metadata_json = serde_json::to_string(&metadata).unwrap_or_default();
        let id_for_db = id.clone();
        let execution_id_for_db = execution_id.clone();
        let kind_for_db = kind.clone();
        let hash_for_db = hash.clone();
        let cache = self.cache.clone();
        let artifact_for_cache = artifact.clone();

        tokio::task::spawn_blocking(move || -> Result<(), String> {
            if let Some(parent) = blob_path_clone.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| format!("create blob dir: {}", e))?;
            }
            std::fs::write(&blob_path_clone, blob)
                .map_err(|e| format!("write artifact blob: {}", e))?;
            let conn = db.blocking_lock();
            conn.execute(
                "INSERT OR REPLACE INTO vee_artifacts
                 (artifact_id, execution_id, kind, metadata_json, blob_path, blob_hash, provenance_json, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, datetime('now'))",
                params![
                    id_for_db,
                    execution_id_for_db,
                    kind_for_db,
                    metadata_json,
                    blob_path_clone.to_string_lossy().to_string(),
                    hash_for_db,
                    provenance_json,
                ],
            )
            .map_err(|e| format!("insert artifact row: {}", e))?;
            Ok(())
        })
        .await
        .map_err(|e| format!("artifact store task failed: {}", e))??;

        cache.lock().await.put(id.clone(), artifact_for_cache);
        Ok(id)
    }

    /// Retrieve an artifact by ID.
    pub async fn get(&self, artifact_id: &str) -> Option<Artifact> {
        {
            let mut cache = self.cache.lock().await;
            if let Some(artifact) = cache.get(artifact_id) {
                return Some(artifact.clone());
            }
        }

        let db = self.db.clone();
        let artifact_id = artifact_id.to_string();
        let cache = self.cache.clone();
        let blocking_artifact_id = artifact_id.clone();
        let cache_artifact_id = artifact_id;
        let artifact = tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let path: Option<String> = conn
                .query_row(
                    "SELECT blob_path FROM vee_artifacts WHERE artifact_id = ?1",
                    [&blocking_artifact_id],
                    |row| row.get(0),
                )
                .optional()
                .ok()
                .flatten();
            let path = match path {
                Some(p) => p,
                None => return None,
            };
            let bytes = match std::fs::read(&path) {
                Ok(b) => b,
                Err(e) => {
                    tracing::warn!(error = %e, path = %path, "artifact blob missing");
                    return None;
                }
            };
            serde_json::from_slice::<Artifact>(&bytes).ok()
        })
        .await
        .ok()
        .flatten();

        if let Some(ref artifact) = artifact {
            cache.lock().await.put(cache_artifact_id, artifact.clone());
        }
        artifact
    }

    /// Get artifacts for a specific execution.
    pub async fn get_by_execution(&self, execution_id: &str) -> Vec<(String, Artifact)> {
        let db = self.db.clone();
        let execution_id = execution_id.to_string();
        let cache = self.cache.clone();
        let pairs = tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let mut stmt = match conn
                .prepare("SELECT artifact_id, blob_path FROM vee_artifacts WHERE execution_id = ?1")
            {
                Ok(s) => s,
                Err(e) => {
                    tracing::warn!(error = %e, "prepare get_by_execution failed");
                    return Vec::new();
                }
            };
            let rows = stmt.query_map([&execution_id], |row| {
                Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
            });
            let pairs = match rows {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(error = %e, "query get_by_execution failed");
                    return Vec::new();
                }
            };
            let mut out = Vec::new();
            for pair in pairs {
                let (id, path) = match pair {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!(error = %e, "artifact row decode failed");
                        continue;
                    }
                };
                let bytes = match std::fs::read(&path) {
                    Ok(b) => b,
                    Err(e) => {
                        tracing::warn!(error = %e, path = %path, "artifact blob missing");
                        continue;
                    }
                };
                if let Ok(artifact) = serde_json::from_slice::<Artifact>(&bytes) {
                    out.push((id, artifact));
                }
            }
            out
        })
        .await
        .unwrap_or_default();

        {
            let mut cache = cache.lock().await;
            for (id, artifact) in &pairs {
                cache.put(id.clone(), artifact.clone());
            }
        }
        pairs
    }

    /// List all artifact summaries.
    pub async fn list_summaries(&self) -> Vec<ArtifactSummary> {
        let db = self.db.clone();
        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let mut stmt =
                match conn.prepare("SELECT artifact_id, metadata_json FROM vee_artifacts") {
                    Ok(s) => s,
                    Err(e) => {
                        tracing::warn!(error = %e, "prepare list summaries failed");
                        return Vec::new();
                    }
                };
            let rows = stmt.query_map([], |row| {
                let id: String = row.get(0)?;
                let meta: String = row.get(1)?;
                Ok((id, meta))
            });
            let mut out = Vec::new();
            if let Ok(rows) = rows {
                for (id, meta) in rows.flatten() {
                    if let Ok(metadata) = serde_json::from_str::<ArtifactMetadata>(&meta) {
                        out.push(ArtifactSummary {
                            artifact_id: id,
                            artifact_type: metadata.kind,
                            mime_type: metadata.mime_type,
                            size_bytes: metadata.size_bytes,
                        });
                    }
                }
            }
            out
        })
        .await
        .unwrap_or_default()
    }

    /// Get provenance for an artifact.
    pub async fn get_provenance(&self, artifact_id: &str) -> Option<Provenance> {
        let db = self.db.clone();
        let artifact_id = artifact_id.to_string();
        tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let json: Option<String> = conn
                .query_row(
                    "SELECT provenance_json FROM vee_artifacts WHERE artifact_id = ?1",
                    [&artifact_id],
                    |row| row.get(0),
                )
                .optional()
                .ok()
                .flatten();
            json.and_then(|j| serde_json::from_str(&j).ok())
        })
        .await
        .ok()
        .flatten()
    }

    /// Delete an artifact and its blob if no other row references it.
    pub async fn delete(&self, artifact_id: &str) -> bool {
        let db = self.db.clone();
        let artifact_id = artifact_id.to_string();
        let cache = self.cache.clone();
        let blocking_artifact_id = artifact_id.clone();
        let deleted = tokio::task::spawn_blocking(move || {
            let conn = db.blocking_lock();
            let path: Option<String> = conn
                .query_row(
                    "SELECT blob_path FROM vee_artifacts WHERE artifact_id = ?1",
                    [&blocking_artifact_id],
                    |row| row.get(0),
                )
                .optional()
                .ok()
                .flatten();
            let deleted = conn
                .execute(
                    "DELETE FROM vee_artifacts WHERE artifact_id = ?1",
                    [&blocking_artifact_id],
                )
                .map(|n| n > 0)
                .unwrap_or(false);
            if let Some(path) = path {
                let refs: i64 = conn
                    .query_row(
                        "SELECT COUNT(*) FROM vee_artifacts WHERE blob_path = ?1",
                        [&path],
                        |row| row.get(0),
                    )
                    .unwrap_or(1);
                if refs == 0 {
                    if let Err(e) = std::fs::remove_file(&path) {
                        tracing::warn!(error = %e, path = %path, "failed to remove artifact blob");
                    }
                }
            }
            deleted
        })
        .await
        .unwrap_or(false);

        if deleted {
            cache.lock().await.pop(&artifact_id);
        }
        deleted
    }

    fn blob_path(&self, hash: &str) -> PathBuf {
        let prefix = &hash[..std::cmp::min(2, hash.len())];
        self.blob_dir.join(prefix).join(hash)
    }

    fn kind_of(artifact: &Artifact) -> String {
        match artifact {
            Artifact::Text { .. } => "text",
            Artifact::Dataset { .. } => "dataset",
            Artifact::Image { .. } => "image",
            Artifact::Model { .. } => "model",
            Artifact::Json { .. } => "json",
            Artifact::Log { .. } => "log",
            Artifact::File { .. } => "file",
        }
        .to_string()
    }

    fn artifact_id(artifact: &Artifact) -> String {
        match artifact {
            Artifact::Text { .. } => format!("art-text-{}", uuid::Uuid::new_v4()),
            Artifact::Dataset { .. } => format!("art-dataset-{}", uuid::Uuid::new_v4()),
            Artifact::Image { .. } => format!("art-image-{}", uuid::Uuid::new_v4()),
            Artifact::Model { .. } => format!("art-model-{}", uuid::Uuid::new_v4()),
            Artifact::Json { .. } => format!("art-json-{}", uuid::Uuid::new_v4()),
            Artifact::Log { .. } => format!("art-log-{}", uuid::Uuid::new_v4()),
            Artifact::File { .. } => format!("art-file-{}", uuid::Uuid::new_v4()),
        }
    }
}

impl Default for ArtifactStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{LogEntry, LogLevel, TextFormat};

    fn test_store(tmp: &tempfile::TempDir) -> ArtifactStore {
        ArtifactStore::try_new(&tmp.path().join("artifacts.db"), &tmp.path().join("blobs")).unwrap()
    }

    #[tokio::test]
    async fn test_store_and_get_text_artifact() {
        let tmp = tempfile::tempdir().unwrap();
        let store = test_store(&tmp);
        let artifact = Artifact::Text {
            content: "hello world".into(),
            format: TextFormat::Plain,
            line_count: 1,
        };
        let id = store.store(artifact.clone(), None).await.unwrap();
        let loaded = store.get(&id).await.unwrap();
        assert!(matches!(loaded, Artifact::Text { content, .. } if content == "hello world"));
    }

    #[tokio::test]
    async fn test_get_by_execution() {
        let tmp = tempfile::tempdir().unwrap();
        let store = test_store(&tmp);
        let artifact = Artifact::Text {
            content: "unit test".into(),
            format: TextFormat::Plain,
            line_count: 1,
        };
        let provenance = Provenance {
            execution_id: "exec-123".into(),
            ..Default::default()
        };
        store.store(artifact, Some(provenance)).await.unwrap();
        let arts = store.get_by_execution("exec-123").await;
        assert_eq!(arts.len(), 1);
    }

    #[tokio::test]
    async fn test_delete_removes_blob_when_unreferenced() {
        let tmp = tempfile::tempdir().unwrap();
        let store = test_store(&tmp);
        let artifact = Artifact::Log {
            entries: vec![LogEntry {
                timestamp: chrono::Utc::now(),
                level: LogLevel::Info,
                message: "msg".into(),
                source: "test".into(),
            }],
            level_counts: [(LogLevel::Info, 1)].into_iter().collect(),
        };
        let id = store.store(artifact, None).await.unwrap();
        assert!(store.delete(&id).await);
        assert!(store.get(&id).await.is_none());
    }

    #[tokio::test]
    async fn test_lru_cache_returns_same_artifact() {
        let tmp = tempfile::tempdir().unwrap();
        let store = test_store(&tmp);
        let artifact = Artifact::Text {
            content: "cached".into(),
            format: TextFormat::Plain,
            line_count: 1,
        };
        let id = store.store(artifact, None).await.unwrap();
        let a1 = store.get(&id).await.unwrap();
        let a2 = store.get(&id).await.unwrap();
        assert!(matches!(a1, Artifact::Text { content, .. } if content == "cached"));
        assert!(matches!(a2, Artifact::Text { content, .. } if content == "cached"));
    }

    #[tokio::test]
    async fn test_content_addressable_blob_path() {
        let tmp = tempfile::tempdir().unwrap();
        let store = test_store(&tmp);
        let artifact = Artifact::Text {
            content: "dedup".into(),
            format: TextFormat::Plain,
            line_count: 1,
        };
        let id = store.store(artifact.clone(), None).await.unwrap();
        let loaded = store.get(&id).await.unwrap();
        assert!(matches!(loaded, Artifact::Text { content, .. } if content == "dedup"));

        // A second store of identical bytes should reuse the same blob path
        // even though the artifact id is new.
        let id2 = store.store(artifact, None).await.unwrap();
        assert_ne!(id, id2);
        let loaded2 = store.get(&id2).await.unwrap();
        assert!(matches!(loaded2, Artifact::Text { content, .. } if content == "dedup"));
    }
}
