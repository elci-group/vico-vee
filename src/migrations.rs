//! SQLite migration runner for `vico-vee`.
//!
//! Migrations are embedded SQL files in `vico-vee/migrations/`. The runner
//! records applied versions in a `vee_migrations` table and only applies
//! migrations newer than the current version. Each SQLite database used by the
//! service (artifacts, checkpoints, patterns) runs the same migration set on
//! startup so that the schema is versioned consistently.
//!
//! Up migrations tolerate duplicate-column and already-exists errors so that
//! existing databases created before migration tracking upgrade cleanly. Down
//! migrations are strict and are used primarily for tests and rollback
//! scenarios.

use rusqlite::{params, Connection};

/// A single numbered schema migration.
#[derive(Debug, Clone)]
pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub sql: &'static str,
    pub down_sql: Option<&'static str>,
}

/// The full migration set, embedded at compile time.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial",
        sql: include_str!("../migrations/001_initial.sql"),
        down_sql: Some(include_str!("../migrations/001_initial.down.sql")),
    },
    Migration {
        version: 2,
        name: "add_artifact_blob_hash",
        sql: include_str!("../migrations/002_add_artifact_blob_hash.sql"),
        down_sql: Some(include_str!(
            "../migrations/002_add_artifact_blob_hash.down.sql"
        )),
    },
];

/// Run all pending migrations up to the latest known version.
pub fn run_migrations(conn: &Connection, migrations: &[Migration]) -> Result<(), String> {
    let target = migrations.iter().map(|m| m.version).max().unwrap_or(0);
    run_migrations_to(conn, migrations, target)
}

/// Migrate the database to the requested target version.
///
/// Runs `up` scripts when moving forward and `down` scripts when moving
/// backward. Applied versions are recorded in `vee_migrations`.
pub fn run_migrations_to(
    conn: &Connection,
    migrations: &[Migration],
    target: i64,
) -> Result<(), String> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS vee_migrations (
            version INTEGER PRIMARY KEY,
            name TEXT NOT NULL,
            applied_at TEXT NOT NULL DEFAULT (datetime('now'))
        )",
        [],
    )
    .map_err(|e| format!("create vee_migrations table: {e}"))?;

    let current_version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM vee_migrations",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("query current schema version: {e}"))?;

    if target > current_version {
        for migration in migrations
            .iter()
            .filter(|m| m.version > current_version && m.version <= target)
        {
            match conn.execute_batch(migration.sql) {
                Ok(()) => {}
                Err(e) => {
                    let msg = e.to_string().to_lowercase();
                    // Allow idempotent re-runs for operations that are already applied.
                    if msg.contains("already exists") || msg.contains("duplicate column name") {
                        tracing::debug!(
                            version = migration.version,
                            name = migration.name,
                            error = %e,
                            "migration step appears already applied, continuing"
                        );
                    } else {
                        return Err(format!(
                            "apply migration {} '{}': {e}",
                            migration.version, migration.name
                        ));
                    }
                }
            }

            conn.execute(
                "INSERT INTO vee_migrations (version, name) VALUES (?1, ?2)",
                params![migration.version, migration.name],
            )
            .map_err(|e| format!("record migration {}: {e}", migration.version))?;
        }
    } else if target < current_version {
        for migration in migrations
            .iter()
            .filter(|m| m.version > target && m.version <= current_version)
            .rev()
        {
            if let Some(down) = migration.down_sql {
                conn.execute_batch(down).map_err(|e| {
                    format!(
                        "revert migration {} '{}': {e}",
                        migration.version, migration.name
                    )
                })?;
            }
            conn.execute(
                "DELETE FROM vee_migrations WHERE version = ?1",
                params![migration.version],
            )
            .map_err(|e| format!("remove migration {} record: {e}", migration.version))?;
        }
    }

    Ok(())
}

/// Return the highest applied migration version, or `0` if none.
pub fn current_version(conn: &Connection) -> Result<i64, String> {
    let version: i64 = conn
        .query_row(
            "SELECT COALESCE(MAX(version), 0) FROM vee_migrations",
            [],
            |row| row.get(0),
        )
        .map_err(|e| format!("query current schema version: {e}"))?;
    Ok(version)
}

/// Convenience builder used by stores that opens a connection and runs the
/// embedded migration set.
#[derive(Debug, Default, Clone)]
pub struct Runner {
    migrations: &'static [Migration],
}

impl Runner {
    /// Create a runner pre-loaded with the embedded `MIGRATIONS` set.
    pub fn new() -> Self {
        Self {
            migrations: MIGRATIONS,
        }
    }

    /// Run all pending migrations on the provided connection.
    pub fn run(&self, conn: &Connection) -> Result<(), String> {
        run_migrations(conn, self.migrations)
    }

    /// Migrate the connection to a specific target version.
    pub fn run_to(&self, conn: &Connection, target: i64) -> Result<(), String> {
        run_migrations_to(conn, self.migrations, target)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_runner_creates_tracking_table() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn, MIGRATIONS).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM vee_migrations", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, MIGRATIONS.len() as i64);
    }

    #[test]
    fn migration_runner_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn, MIGRATIONS).unwrap();
        run_migrations(&conn, MIGRATIONS).unwrap();

        let version: i64 = conn
            .query_row("SELECT MAX(version) FROM vee_migrations", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!(version, MIGRATIONS.last().unwrap().version);
    }

    #[test]
    fn migration_runner_creates_expected_tables() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn, MIGRATIONS).unwrap();

        let tables: Vec<String> = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name")
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert!(tables.contains(&"patterns".to_string()));
        assert!(tables.contains(&"vee_artifacts".to_string()));
        assert!(tables.contains(&"vee_checkpoints".to_string()));
        assert!(tables.contains(&"vee_migrations".to_string()));
    }

    #[test]
    fn migration_runner_ignores_already_applied_column() {
        let conn = Connection::open_in_memory().unwrap();
        conn.execute(
            "CREATE TABLE vee_artifacts (
                artifact_id TEXT PRIMARY KEY,
                execution_id TEXT NOT NULL,
                kind TEXT NOT NULL,
                metadata_json TEXT NOT NULL,
                blob_path TEXT NOT NULL,
                blob_hash TEXT,
                provenance_json TEXT,
                created_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )
        .unwrap();

        // Should not fail even though blob_hash already exists.
        run_migrations(&conn, MIGRATIONS).unwrap();
    }

    #[test]
    fn migration_runner_down_reverts_to_target() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn, MIGRATIONS).unwrap();
        assert_eq!(
            current_version(&conn).unwrap(),
            MIGRATIONS.last().unwrap().version
        );

        run_migrations_to(&conn, MIGRATIONS, 0).unwrap();
        assert_eq!(current_version(&conn).unwrap(), 0);
    }

    #[test]
    fn migration_runner_down_then_up_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        run_migrations(&conn, MIGRATIONS).unwrap();
        run_migrations_to(&conn, MIGRATIONS, 0).unwrap();
        run_migrations(&conn, MIGRATIONS).unwrap();

        assert_eq!(
            current_version(&conn).unwrap(),
            MIGRATIONS.last().unwrap().version
        );
    }

    #[test]
    fn migrated_stores_can_read_and_write() {
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
}
