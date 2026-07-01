use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

use super::schema::ColumnDef;

// ─────────────────────────────────────────────────────────────────────────────
// Artifacts
// ─────────────────────────────────────────────────────────────────────────────

/// Strongly-typed execution output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum Artifact {
    /// Plain or structured text
    Text {
        content: String,
        format: TextFormat,
        line_count: usize,
    },
    /// Tabular data
    Dataset {
        schema: Vec<ColumnDef>,
        row_count: usize,
        sample: Vec<serde_json::Value>,
        memory_bytes: usize,
    },
    /// Static image
    Image {
        format: ImageFormat,
        width: u32,
        height: u32,
        bytes: Vec<u8>,
    },
    /// Trained ML model
    Model {
        framework: String,
        format: ModelFormat,
        parameters: usize,
        metrics: HashMap<String, f64>,
    },
    /// Structured JSON
    Json {
        value: serde_json::Value,
        schema_hash: String,
    },
    /// Execution log
    Log {
        entries: Vec<LogEntry>,
        level_counts: HashMap<LogLevel, usize>,
    },
    /// File reference (large files stored separately)
    File {
        path: PathBuf,
        size_bytes: u64,
        mime_type: String,
        hash: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TextFormat {
    Markdown,
    Json,
    Yaml,
    Csv,
    Plain,
    Html,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ImageFormat {
    Png,
    Jpg,
    Svg,
    Webp,
    Gif,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ModelFormat {
    Onnx,
    Pickle,
    Safetensors,
    Json,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LogEntry {
    pub timestamp: DateTime<Utc>,
    pub level: LogLevel,
    pub message: String,
    pub source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Hash, Eq, PartialEq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

/// Summary of an artifact for event streaming.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactSummary {
    pub artifact_id: String,
    pub artifact_type: String,
    pub mime_type: Option<String>,
    pub size_bytes: u64,
}

impl From<&Artifact> for ArtifactSummary {
    fn from(artifact: &Artifact) -> Self {
        match artifact {
            Artifact::Text { content, .. } => ArtifactSummary {
                artifact_id: format!("art-text-{}", uuid::Uuid::new_v4()),
                artifact_type: "text".into(),
                mime_type: Some("text/plain".into()),
                size_bytes: content.len() as u64,
            },
            Artifact::Dataset { row_count: _, .. } => ArtifactSummary {
                artifact_id: format!("art-dataset-{}", uuid::Uuid::new_v4()),
                artifact_type: "dataset".into(),
                mime_type: Some("application/json".into()),
                size_bytes: 0,
            },
            Artifact::Image { bytes, .. } => ArtifactSummary {
                artifact_id: format!("art-image-{}", uuid::Uuid::new_v4()),
                artifact_type: "image".into(),
                mime_type: Some("image/png".into()),
                size_bytes: bytes.len() as u64,
            },
            Artifact::Model { .. } => ArtifactSummary {
                artifact_id: format!("art-model-{}", uuid::Uuid::new_v4()),
                artifact_type: "model".into(),
                mime_type: Some("application/octet-stream".into()),
                size_bytes: 0,
            },
            Artifact::Json { value, .. } => ArtifactSummary {
                artifact_id: format!("art-json-{}", uuid::Uuid::new_v4()),
                artifact_type: "json".into(),
                mime_type: Some("application/json".into()),
                size_bytes: value.to_string().len() as u64,
            },
            Artifact::Log { entries, .. } => ArtifactSummary {
                artifact_id: format!("art-log-{}", uuid::Uuid::new_v4()),
                artifact_type: "log".into(),
                mime_type: Some("text/plain".into()),
                size_bytes: entries.iter().map(|e| e.message.len()).sum::<usize>() as u64,
            },
            Artifact::File {
                size_bytes,
                mime_type,
                ..
            } => ArtifactSummary {
                artifact_id: format!("art-file-{}", uuid::Uuid::new_v4()),
                artifact_type: "file".into(),
                mime_type: Some(mime_type.clone()),
                size_bytes: *size_bytes,
            },
        }
    }
}
