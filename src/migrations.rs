//! SQLite migration runner for `vico-vee`.
//!
//! Migrations are numbered SQL files applied in ascending order. Applied
//! migrations are tracked in the `vee_migrations` table.

use rusqlite::{params, Connection};
use std::path::Path;

/// A single migration script.
#[derive(Debug, Clone)]
pub struct Migration {
    pub name: String,
    pub sql: String,
}

/// Migration runner that applies a set of migrations to a SQLite connection.
#[derive(Debug, Default, Clone)]
pub struct Runner {
    migrations: Vec<Migration>,
}

impl Runner {
    /// Create a new runner with the embedded initial migration.
    pub fn new() -> Self {
        Self {
            migrations: vec![Migration {
                name: "001_initial".into(),
                sql: include_str!("../migrations/001_initial.sql").into(),
            }],
        }
    }

    /// Load all `.sql` migrations from a directory and append them to the runner.
    ///
    /// Files are sorted lexicographically; the convention is `001_initial.sql`,
    /// `002_add_indices.sql`, etc.
    pub fn with_dir(mut self, dir: &Path) -> Result<Self, String> {
        let mut entries: Vec<_> = std::fs::read_dir(dir)
            .map_err(|e| format!("read migrations dir: {e}"))?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|ext| ext == "sql"))
            .collect();
        entries.sort();

        for path in entries {
            let name = path
                .file_stem()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
                .ok_or_else(|| format!("invalid migration filename: {}", path.display()))?;
            let sql = std::fs::read_to_string(&path)
                .map_err(|e| format!("read migration {}: {e}", path.display()))?;
            self.migrations.push(Migration { name, sql });
        }
        Ok(self)
    }

    /// Apply the configured migrations to an open SQLite connection.
    ///
    /// Creates the `vee_migrations` tracking table if it does not exist and skips
    /// already-applied migrations.
    pub fn run(&self, conn: &Connection) -> Result<(), String> {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS vee_migrations (
                name TEXT PRIMARY KEY,
                applied_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            [],
        )
        .map_err(|e| format!("create vee_migrations table: {e}"))?;

        let applied: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT name FROM vee_migrations ORDER BY name")
                .map_err(|e| format!("prepare applied migrations query: {e}"))?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0));
            rows.map_err(|e| format!("query applied migrations: {e}"))?
                .filter_map(|r| r.ok())
                .collect()
        };

        for migration in &self.migrations {
            if applied.contains(&migration.name) {
                continue;
            }
            conn.execute_batch(&migration.sql)
                .map_err(|e| format!("apply migration {}: {e}", migration.name))?;
            conn.execute(
                "INSERT INTO vee_migrations (name, applied_at) VALUES (?1, datetime('now'))",
                params![&migration.name],
            )
            .map_err(|e| format!("record migration {}: {e}", migration.name))?;
            tracing::info!(migration = %migration.name, "applied migration");
        }

        Ok(())
    }

    /// Return the highest applied migration name tracked in `vee_migrations`.
    pub fn current_version(&self, conn: &Connection) -> Result<Option<String>, String> {
        let version: Option<String> = conn
            .query_row(
                "SELECT name FROM vee_migrations ORDER BY name DESC LIMIT 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(|e| format!("query current version: {e}"))?;
        Ok(version)
    }
}

/// Load all `.sql` migrations from a directory without a runner.
pub fn load_from_dir(dir: &Path) -> Result<Vec<Migration>, String> {
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .map_err(|e| format!("read migrations dir: {e}"))?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "sql"))
        .collect();
    entries.sort();

    let mut migrations = Vec::new();
    for path in entries {
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(|s| s.to_string())
            .ok_or_else(|| format!("invalid migration filename: {}", path.display()))?;
        let sql = std::fs::read_to_string(&path)
            .map_err(|e| format!("read migration {}: {e}", path.display()))?;
        migrations.push(Migration { name, sql });
    }
    Ok(migrations)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_applied_migrations() {
        let conn = Connection::open_in_memory().unwrap();
        Runner::new()
            .run(&conn)
            .expect("initial migration should apply");

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM vee_migrations", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 1);

        // Re-applying is idempotent.
        Runner::new().run(&conn).expect("re-apply should be idempotent");
        let count2: i64 = conn
            .query_row("SELECT COUNT(*) FROM vee_migrations", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count2, 1);
    }

    #[test]
    fn current_version_reflects_latest_migration() {
        let conn = Connection::open_in_memory().unwrap();
        let runner = Runner::new();
        assert_eq!(runner.current_version(&conn).unwrap(), None);

        runner.run(&conn).unwrap();
        assert_eq!(runner.current_version(&conn).unwrap(), Some("001_initial".into()));
    }

    #[test]
    fn migration_creates_expected_tables() {
        let conn = Connection::open_in_memory().unwrap();
        Runner::new().run(&conn).unwrap();

        let tables: Vec<String> = conn
            .prepare(
                "SELECT name FROM sqlite_master WHERE type = 'table' AND name IN (
                    'vee_artifacts', 'vee_checkpoints', 'patterns', 'vee_revoked_capabilities', 'vee_migrations'
                )",
            )
            .unwrap()
            .query_map([], |row| row.get::<_, String>(0))
            .unwrap()
            .filter_map(|r| r.ok())
            .collect();

        assert_eq!(tables.len(), 5);
    }
}
