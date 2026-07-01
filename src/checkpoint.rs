//! Durable Execution — SQLite Checkpoints
//!
//! Before each phase (Hypothesis, Simulation, Execution, Validation),
//! state is checkpointed to disk. On crash or daemon restart,
//! executions resume from their last checkpoint.
//!
//! This is a lightweight adaptation of Temporal.io's durable
//! execution pattern, using SQLite instead of a full orchestrator.

use crate::types::*;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::PathBuf;

/// A checkpoint captures execution state at a phase boundary.
#[derive(Debug, Clone)]
pub struct Checkpoint {
    pub checkpoint_id: String,
    pub execution_id: String,
    pub phase: ExecutionPhase,
    pub status: ExecutionStatus,
    pub artifacts_json: String,
    pub validation_json: Option<String>,
    pub error_log: Option<String>,
    pub confidence: f64,
    pub tokens_consumed: u64,
    pub cpu_seconds_used: f64,
    pub memory_peak_mb: f64,
    pub created_at: String,
}

/// SQLite-backed checkpoint store.
pub struct CheckpointStore {
    conn: Connection,
}

impl CheckpointStore {
    pub fn new(db_path: &PathBuf) -> Result<Self, String> {
        let conn = Connection::open(db_path).map_err(|e| format!("open checkpoint db: {}", e))?;
        conn.execute(
            "CREATE TABLE IF NOT EXISTS vee_checkpoints (
                checkpoint_id TEXT PRIMARY KEY,
                execution_id TEXT NOT NULL,
                phase TEXT NOT NULL,
                status TEXT NOT NULL,
                artifacts_json TEXT NOT NULL DEFAULT '[]',
                validation_json TEXT,
                error_log TEXT,
                confidence REAL NOT NULL DEFAULT 0.0,
                tokens_consumed INTEGER NOT NULL DEFAULT 0,
                cpu_seconds_used REAL NOT NULL DEFAULT 0.0,
                memory_peak_mb REAL NOT NULL DEFAULT 0.0,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )
        .map_err(|e| format!("create checkpoints table: {}", e))?;

        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_ckpt_exec ON vee_checkpoints(execution_id)",
            [],
        )
        .map_err(|e| format!("create index: {}", e))?;

        Ok(Self { conn })
    }

    /// Save a checkpoint.
    pub fn save(&self, ckpt: &Checkpoint) -> Result<(), String> {
        self.conn
            .execute(
                "INSERT OR REPLACE INTO vee_checkpoints (
                checkpoint_id, execution_id, phase, status, artifacts_json,
                validation_json, error_log, confidence, tokens_consumed,
                cpu_seconds_used, memory_peak_mb, created_at
            ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                params![
                    ckpt.checkpoint_id,
                    ckpt.execution_id,
                    format!("{:?}", ckpt.phase),
                    format!("{:?}", ckpt.status),
                    ckpt.artifacts_json,
                    ckpt.validation_json,
                    ckpt.error_log,
                    ckpt.confidence,
                    ckpt.tokens_consumed,
                    ckpt.cpu_seconds_used,
                    ckpt.memory_peak_mb,
                    ckpt.created_at,
                ],
            )
            .map_err(|e| format!("save checkpoint: {}", e))?;
        Ok(())
    }

    /// Get the most recent checkpoint for an execution.
    pub fn get_latest(&self, execution_id: &str) -> Option<Checkpoint> {
        self.conn
            .query_row(
                "SELECT checkpoint_id, execution_id, phase, status, artifacts_json,
                    validation_json, error_log, confidence, tokens_consumed,
                    cpu_seconds_used, memory_peak_mb, created_at
             FROM vee_checkpoints
             WHERE execution_id = ?1
             ORDER BY created_at DESC
             LIMIT 1",
                [execution_id],
                |row| {
                    Ok(Checkpoint {
                        checkpoint_id: row.get(0)?,
                        execution_id: row.get(1)?,
                        phase: parse_phase(&row.get::<_, String>(2)?),
                        status: parse_status(&row.get::<_, String>(3)?),
                        artifacts_json: row.get(4)?,
                        validation_json: row.get(5)?,
                        error_log: row.get(6)?,
                        confidence: row.get(7)?,
                        tokens_consumed: row.get(8)?,
                        cpu_seconds_used: row.get(9)?,
                        memory_peak_mb: row.get(10)?,
                        created_at: row.get(11)?,
                    })
                },
            )
            .optional()
            .ok()
            .flatten()
    }

    /// List all execution IDs that have checkpoints (for resumption).
    pub fn list_incomplete(&self) -> Result<Vec<String>, String> {
        let mut stmt = self
            .conn
            .prepare(
                "SELECT DISTINCT execution_id FROM vee_checkpoints
             WHERE status NOT IN ('Completed', 'Failed', 'Cancelled')",
            )
            .map_err(|e| format!("prepare: {}", e))?;

        let rows = stmt
            .query_map([], |row| row.get(0))
            .map_err(|e| format!("query_map: {}", e))?;

        let mut result = Vec::new();
        for row in rows {
            result.push(row.map_err(|e| format!("row: {}", e))?);
        }
        Ok(result)
    }

    /// Delete all checkpoints for an execution.
    pub fn delete_for_execution(&self, execution_id: &str) -> Result<(), String> {
        self.conn
            .execute(
                "DELETE FROM vee_checkpoints WHERE execution_id = ?1",
                [execution_id],
            )
            .map_err(|e| format!("delete checkpoints: {}", e))?;
        Ok(())
    }

    /// Count checkpoints.
    pub fn count(&self) -> Result<i64, String> {
        self.conn
            .query_row("SELECT COUNT(*) FROM vee_checkpoints", [], |row| row.get(0))
            .map_err(|e| format!("count: {}", e))
    }
}

fn parse_phase(s: &str) -> ExecutionPhase {
    match s {
        "Hypothesis" => ExecutionPhase::Hypothesis,
        "Simulation" => ExecutionPhase::Simulation,
        "Execution" => ExecutionPhase::Execution,
        "Validation" => ExecutionPhase::Validation,
        "Feedback" => ExecutionPhase::Feedback,
        _ => ExecutionPhase::Hypothesis,
    }
}

fn parse_status(s: &str) -> ExecutionStatus {
    match s {
        "Pending" => ExecutionStatus::Pending,
        "Queued" => ExecutionStatus::Queued,
        "Simulating" => ExecutionStatus::Simulating,
        "Executing" => ExecutionStatus::Executing,
        "Validating" => ExecutionStatus::Validating,
        "Completed" => ExecutionStatus::Completed,
        "Failed" => ExecutionStatus::Failed,
        "Recovered" => ExecutionStatus::Recovered,
        "Cancelled" => ExecutionStatus::Cancelled,
        _ => ExecutionStatus::Pending,
    }
}

/// Build a checkpoint from the current execution state.
pub fn checkpoint_from_result(
    execution_id: &str,
    phase: ExecutionPhase,
    result: &ExecutionResult,
) -> Checkpoint {
    Checkpoint {
        checkpoint_id: format!(
            "ckpt-{}-{}",
            execution_id,
            chrono::Utc::now().timestamp_millis()
        ),
        execution_id: execution_id.to_string(),
        phase,
        status: result.status.clone(),
        artifacts_json: serde_json::to_string(&result.artifacts).unwrap_or_else(|_| "[]".into()),
        validation_json: result
            .validation
            .as_ref()
            .map(|v| serde_json::to_string(v).unwrap_or_default()),
        error_log: result.error_log.clone(),
        confidence: result.confidence,
        tokens_consumed: result.tokens_consumed,
        cpu_seconds_used: result.cpu_seconds_used,
        memory_peak_mb: result.memory_peak_mb,
        created_at: chrono::Utc::now().to_rfc3339(),
    }
}
