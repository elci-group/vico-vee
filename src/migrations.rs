//! SQLite migration runner for `vico-vee`.
//!
//! Migrations are embedded SQL files in `vico-vee/migrations/`. The runner
//! records applied versions in a `vee_migrations` table and only applies
//! migrations newer than the current version. Each SQLite database used by the
//! service (artifacts, checkpoints, patterns) runs the same migration set on
//! startup so that the schema is versioned consistently.

use rusqlite::{params, Connection};

/// A single numbered migration.
#[derive(Debug, Clone)]
pub struct Migration {
    pub version: i64,
    pub name: &'static str,
    pub sql: &'static str,
}

/// The full migration set, embedded at compile time.
pub const MIGRATIONS: &[Migration] = &[
    Migration {
        version: 1,
        name: "initial",
        sql: include_str!("../migrations/001_initial.sql"),
    },
    Migration {
        version: 2,
        name: "add_artifact_blob_hash",
        sql: include_str!("../migrations/002_add_artifact_blob_hash.sql"),
    },
];

/// Migration runner.
#[derive(Debug, Clone, Default)]
pub struct Runner;

impl Runner {
    /// Create a new runner configured with the embedded migration set.
    pub fn new() -> Self {
        Self
    }

    /// Run all pending migrations on the given SQLite connection.
    ///
    /// Creates the `vee_migrations` tracking table if it does not yet exist and
    /// updates it as each migration is applied. Individual statements that fail
    /// because an object already exists (for example a duplicate column add on a
    /// database that was created before migration tracking) are treated as no-ops
    /// so that the runner remains idempotent for existing deployments.
    pub fn run(&self, conn: &Connection) -> Result<(), String> {
        run_migrations(conn, MIGRATIONS)
    }
}

/// Run a set of migrations on the given SQLite connection.
pub fn run_migrations(conn: &Connection, migrations: &[Migration]) -> Result<(), String> {
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

    for migration in migrations {
        if migration.version <= current_version {
            continue;
        }

        match conn.execute(migration.sql, []) {
            Ok(_) => {}
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
                        "migration {} '{}': {e}",
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

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_runner_creates_tracking_table() {
        let conn = Connection::open_in_memory().unwrap();
        Runner::new().run(&conn).unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM vee_migrations", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, MIGRATIONS.len() as i64);
    }

    #[test]
    fn migration_runner_is_idempotent() {
        let conn = Connection::open_in_memory().unwrap();
        Runner::new().run(&conn).unwrap();
        Runner::new().run(&conn).unwrap();

        let version: i64 = conn
            .query_row(
                "SELECT MAX(version) FROM vee_migrations",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(version, MIGRATIONS.last().unwrap().version);
    }

    #[test]
    fn migration_runner_creates_expected_tables() {
        let conn = Connection::open_in_memory().unwrap();
        Runner::new().run(&conn).unwrap();

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
        Runner::new().run(&conn).unwrap();
    }
}
