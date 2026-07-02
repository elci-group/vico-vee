//! Persistent store for execution metadata.
//!
//! Each [`ExecutionResult`] is serialized to JSON and stored in SQLite so that
//! the daemon's in-memory state can be rebuilt after a restart.

use crate::types::ExecutionResult;
use rusqlite::{params, Connection, OptionalExtension};
use std::path::Path;

/// SQLite-backed execution result store.
pub struct ExecutionStore {
    conn: Connection,
}

impl ExecutionStore {
    /// Open the store at `path`, creating the schema if necessary.
    pub fn new(path: &Path) -> Result<Self, String> {
        let conn = Connection::open(path).map_err(|e| format!("open execution db: {e}"))?;
        crate::migrations::run_migrations(&conn, crate::migrations::MIGRATIONS)
            .map_err(|e| format!("run execution schema migrations: {e}"))?;
        Ok(Self { conn })
    }

    /// Persist an execution result, replacing any existing row.
    pub fn save(&self, project_id: &str, result: &ExecutionResult) -> Result<(), String> {
        let result_json =
            serde_json::to_string(result).map_err(|e| format!("serialize result: {e}"))?;
        let status = format!("{:?}", result.status);
        self.conn
            .execute(
                "INSERT INTO vee_executions
                 (execution_id, project_id, status, result_json, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(execution_id) DO UPDATE SET
                   project_id = excluded.project_id,
                   status = excluded.status,
                   result_json = excluded.result_json,
                   updated_at = excluded.updated_at",
                params![
                    result.execution_id,
                    project_id,
                    status,
                    result_json,
                    result.created_at.to_rfc3339(),
                    chrono::Utc::now().to_rfc3339(),
                ],
            )
            .map_err(|e| format!("save execution: {e}"))?;
        Ok(())
    }

    /// Load the latest state of a single execution, if it exists.
    pub fn load(&self, execution_id: &str) -> Result<Option<ExecutionResult>, String> {
        self.conn
            .query_row(
                "SELECT result_json FROM vee_executions WHERE execution_id = ?1",
                [execution_id],
                |row| {
                    let json: String = row.get(0)?;
                    Ok(json)
                },
            )
            .optional()
            .map_err(|e| format!("load execution: {e}"))?
            .map(|json| {
                serde_json::from_str(&json).map_err(|e| format!("deserialize result: {e}"))
            })
            .transpose()
    }

    /// Load all persisted executions.
    pub fn load_all(&self) -> Result<Vec<ExecutionResult>, String> {
        let mut stmt = self
            .conn
            .prepare("SELECT result_json FROM vee_executions")
            .map_err(|e| format!("prepare load_all: {e}"))?;
        let rows = stmt
            .query_map([], |row| {
                let json: String = row.get(0)?;
                Ok(json)
            })
            .map_err(|e| format!("query load_all: {e}"))?;
        let mut results = Vec::new();
        for row in rows {
            let json = row.map_err(|e| format!("row: {e}"))?;
            let result: ExecutionResult =
                serde_json::from_str(&json).map_err(|e| format!("deserialize result: {e}"))?;
            results.push(result);
        }
        Ok(results)
    }

    /// Return true if the underlying SQLite connection is usable.
    pub fn ping(&self) -> Result<(), String> {
        self.conn
            .query_row("SELECT 1", [], |_row| Ok(()))
            .map(|_| ())
            .map_err(|e| format!("ping execution store: {e}"))
    }

    /// Delete an execution record.
    #[allow(dead_code)]
    pub fn delete(&self, execution_id: &str) -> Result<(), String> {
        self.conn
            .execute(
                "DELETE FROM vee_executions WHERE execution_id = ?1",
                [execution_id],
            )
            .map_err(|e| format!("delete execution: {e}"))?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ExecutionResult, ExecutionStatus};
    use chrono::Utc;

    #[test]
    fn round_trip_execution_result() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ExecutionStore::new(&tmp.path().join("exec.db")).unwrap();

        let result = ExecutionResult {
            execution_id: "exec-1".into(),
            project_id: Some("default".into()),
            status: ExecutionStatus::Completed,
            phase: crate::types::ExecutionPhase::Execution,
            artifacts: vec![],
            validation: None,
            confidence: 0.95,
            tokens_consumed: 42,
            cpu_seconds_used: 1.5,
            memory_peak_mb: 64.0,
            latency_ms: 120,
            error_log: None,
            created_at: Utc::now(),
            started_at: Some(Utc::now()),
            completed_at: Some(Utc::now()),
        };

        store.save("default", &result).unwrap();
        let loaded = store.load("exec-1").unwrap().unwrap();
        assert_eq!(loaded.execution_id, result.execution_id);
        assert_eq!(loaded.status, result.status);
        assert_eq!(loaded.confidence, result.confidence);
    }

    #[test]
    fn load_all_returns_all_rows() {
        let tmp = tempfile::tempdir().unwrap();
        let store = ExecutionStore::new(&tmp.path().join("exec.db")).unwrap();

        for i in 0..3 {
            let result = ExecutionResult {
                execution_id: format!("exec-{i}"),
                project_id: None,
                status: ExecutionStatus::Completed,
                phase: crate::types::ExecutionPhase::Execution,
                artifacts: vec![],
                validation: None,
                confidence: 0.0,
                tokens_consumed: 0,
                cpu_seconds_used: 0.0,
                memory_peak_mb: 0.0,
                latency_ms: 0,
                error_log: None,
                created_at: Utc::now(),
                started_at: None,
                completed_at: None,
            };
            store.save("default", &result).unwrap();
        }

        let all = store.load_all().unwrap();
        assert_eq!(all.len(), 3);
    }
}
