//! Schema migrations for vico-vee SQLite databases.
//!
//! Migrations are embedded at compile time so the released binary is
//! self-contained.  The runner tracks applied versions in `vee_migrations`
//! and supports both up and down migrations.

use rusqlite::{params, Connection};

const MIGRATIONS_TABLE_SQL: &str = r#"
CREATE TABLE IF NOT EXISTS vee_migrations (
    version INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    applied_at TEXT NOT NULL DEFAULT (datetime('now'))
)
"#;

/// A single numbered schema migration.
#[derive(Debug, Clone)]
pub struct Migration {
    pub version: u32,
    pub name: &'static str,
    pub up_sql: &'static str,
    pub down_sql: Option<&'static str>,
}

/// SQLite migration runner.
#[derive(Debug, Clone, Default)]
pub struct Runner {
    migrations: Vec<Migration>,
}

impl Runner {
    /// Create a runner with the built-in migration set.
    pub fn new() -> Self {
        Self {
            migrations: vec![Migration {
                version: 1,
                name: "initial",
                up_sql: include_str!("../migrations/001_initial.sql"),
                down_sql: Some(include_str!("../migrations/001_initial.down.sql")),
            }],
        }
    }

    /// Ensure the migration tracking table exists.
    fn ensure_tracking_table(&self, conn: &Connection) -> Result<(), String> {
        conn.execute(MIGRATIONS_TABLE_SQL, [])
            .map_err(|e| format!("create vee_migrations table: {e}"))?;
        Ok(())
    }

    /// Return the highest known migration version.
    pub fn latest_version(&self) -> u32 {
        self.migrations
            .iter()
            .map(|m| m.version)
            .max()
            .unwrap_or(0)
    }

    /// Return the currently recorded schema version.
    pub fn current_version(&self, conn: &Connection) -> Result<u32, String> {
        self.ensure_tracking_table(conn)?;
        let version: Option<u32> = conn
            .query_row("SELECT MAX(version) FROM vee_migrations", [], |row| {
                row.get(0)
            })
            .optional()
            .map_err(|e| format!("query current schema version: {e}"))?
            .flatten();
        Ok(version.unwrap_or(0))
    }

    /// Run all pending migrations up to `latest_version()`.
    pub fn run(&self, conn: &Connection) -> Result<u32, String> {
        self.run_to(conn, self.latest_version())
    }

    /// Migrate the database to the requested target version.
    ///
    /// Runs `up` scripts when moving forward and `down` scripts when moving
    /// backward.  Each migration is applied inside a single batch; callers may
    /// wrap this in a transaction if they need stricter atomicity.
    pub fn run_to(&self, conn: &Connection, target: u32) -> Result<u32, String> {
        let current = self.current_version(conn)?;

        if target > current {
            for migration in self
                .migrations
                .iter()
                .filter(|m| m.version > current && m.version <= target)
            {
                conn.execute_batch(migration.up_sql)
                    .map_err(|e| {
                        format!(
                            "apply migration {} ({}): {e}",
                            migration.version, migration.name
                        )
                    })?;
                conn.execute(
                    "INSERT INTO vee_migrations (version, name, applied_at) VALUES (?1, ?2, datetime('now'))",
                    params![migration.version, migration.name],
                )
                .map_err(|e| format!("record migration {}: {e}", migration.version))?;
            }
        } else if target < current {
            for migration in self
                .migrations
                .iter()
                .filter(|m| m.version > target && m.version <= current)
                .rev()
            {
                let down = migration.down_sql.ok_or_else(|| {
                    format!("migration {} has no down script", migration.version)
                })?;
                conn.execute_batch(down).map_err(|e| {
                    format!(
                        "revert migration {} ({}): {e}",
                        migration.version, migration.name
                    )
                })?;
                conn.execute(
                    "DELETE FROM vee_migrations WHERE version = ?1",
                    params![migration.version],
                )
                .map_err(|e| format!("remove migration {} record: {e}", migration.version))?;
            }
        }

        Ok(target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    fn in_memory_conn() -> Connection {
        Connection::open_in_memory().unwrap()
    }

    #[test]
    fn runner_creates_tracking_table_and_applies_initial() {
        let conn = in_memory_conn();
        let runner = Runner::new();

        let version = runner.run(&conn).unwrap();
        assert_eq!(version, 1);
        assert_eq!(runner.current_version(&conn).unwrap(), 1);

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM vee_migrations", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);
    }

    #[test]
    fn runner_is_idempotent() {
        let conn = in_memory_conn();
        let runner = Runner::new();

        runner.run(&conn).unwrap();
        runner.run(&conn).unwrap();

        assert_eq!(runner.current_version(&conn).unwrap(), 1);
    }

    #[test]
    fn runner_down_reverts_initial_schema() {
        let conn = in_memory_conn();
        let runner = Runner::new();

        runner.run(&conn).unwrap();
        runner.run_to(&conn, 0).unwrap();

        assert_eq!(runner.current_version(&conn).unwrap(), 0);
    }

    #[tokio::test]
    async fn migrated_artifact_store_operations_work() {
        use crate::artifact::ArtifactStore;
        use crate::types::{Artifact, TextFormat};

        let tmp = tempfile::tempdir().unwrap();
        let store = ArtifactStore::try_new(
            &tmp.path().join("vee_artifacts.db"),
            &tmp.path().join("blobs"),
        )
        .unwrap();

        let id = store
            .store(
                Artifact::Text {
                    content: "migrated".into(),
                    format: TextFormat::Plain,
                    line_count: 1,
                },
                None,
            )
            .await
            .unwrap();

        assert!(store.get(&id).await.is_some());
    }

    #[test]
    fn migrated_checkpoint_store_operations_work() {
        use crate::checkpoint::{Checkpoint, CheckpointStore};
        use crate::types::{ExecutionPhase, ExecutionStatus};

        let tmp = tempfile::tempdir().unwrap();
        let store = CheckpointStore::new(&tmp.path().join("vee_checkpoints.db")).unwrap();

        let ckpt = Checkpoint {
            checkpoint_id: "c1".into(),
            execution_id: "e1".into(),
            phase: ExecutionPhase::Hypothesis,
            status: ExecutionStatus::Pending,
            artifacts_json: "[]".into(),
            validation_json: None,
            error_log: None,
            confidence: 0.0,
            tokens_consumed: 0,
            cpu_seconds_used: 0.0,
            memory_peak_mb: 0.0,
            created_at: chrono::Utc::now().to_rfc3339(),
        };

        store.save(&ckpt).unwrap();
        assert_eq!(store.count().unwrap(), 1);
    }

    #[test]
    fn migrated_pattern_store_operations_work() {
        use crate::pattern::PatternStore;

        let tmp = tempfile::tempdir().unwrap();
        let store = PatternStore::new_with_path(&tmp.path().join("patterns.db")).unwrap();

        // Built-in patterns are seeded when the persistent store is empty.
        assert!(!store.list(None).is_empty());
    }
}
