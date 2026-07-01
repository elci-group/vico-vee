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

/// Load all `.sql` migrations from a directory.
///
/// Files are sorted lexicographically; the convention is `001_initial.sql`,
/// `002_add_indices.sql`, etc.
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

/// Return the embedded initial migration used by the standalone binary.
pub fn embedded_initial() -> Migration {
    Migration {
        name: "001_initial".into(),
        sql: include_str!("../migrations/001_initial.sql").into(),
    }
}

/// Apply a list of migrations to an open SQLite connection.
///
/// Creates the `vee_migrations` tracking table if it does not exist and skips
/// already-applied migrations.
pub fn apply(conn: &mut Connection, migrations: &[Migration]) -> Result<(), String> {
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

    let tx = conn
        .transaction()
        .map_err(|e| format!("begin migration transaction: {e}"))?;
    for migration in migrations {
        if applied.contains(&migration.name) {
            continue;
        }
        tx.execute_batch(&migration.sql)
            .map_err(|e| format!("apply migration {}: {e}", migration.name))?;
        tx.execute(
            "INSERT INTO vee_migrations (name, applied_at) VALUES (?1, datetime('now'))",
            params![&migration.name],
        )
        .map_err(|e| format!("record migration {}: {e}", migration.name))?;
        tracing::info!(migration = %migration.name, "applied migration");
    }
    tx.commit().map_err(|e| format!("commit migrations: {e}"))?;

    Ok(())
}

/// Apply the embedded initial migration set.
pub fn apply_embedded(conn: &mut Connection) -> Result<(), String> {
    apply(conn, &[embedded_initial()])
}

/// Return the highest applied migration name, if any.
pub fn current_version(conn: &Connection) -> Result<Option<String>, String> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracks_applied_migrations() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply(
            &mut conn,
            &[
                Migration {
                    name: "001_a".into(),
                    sql: "CREATE TABLE t1 (id INTEGER PRIMARY KEY);".into(),
                },
                Migration {
                    name: "002_b".into(),
                    sql: "CREATE TABLE t2 (id INTEGER PRIMARY KEY);".into(),
                },
            ],
        )
        .unwrap();

        let count: i64 = conn
            .query_row("SELECT COUNT(*) FROM vee_migrations", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 2);

        // Re-applying is idempotent.
        apply(
            &mut conn,
            &[
                Migration {
                    name: "001_a".into(),
                    sql: "CREATE TABLE t1 (id INTEGER PRIMARY KEY);".into(),
                },
                Migration {
                    name: "002_b".into(),
                    sql: "CREATE TABLE t2 (id INTEGER PRIMARY KEY);".into(),
                },
            ],
        )
        .unwrap();

        let count2: i64 = conn
            .query_row("SELECT COUNT(*) FROM vee_migrations", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count2, 2);
    }

    #[test]
    fn current_version_reflects_latest_migration() {
        let mut conn = Connection::open_in_memory().unwrap();
        assert_eq!(current_version(&conn).unwrap(), None);

        apply(
            &mut conn,
            &[Migration {
                name: "003_c".into(),
                sql: "CREATE TABLE t3 (id INTEGER PRIMARY KEY);".into(),
            }],
        )
        .unwrap();

        assert_eq!(current_version(&conn).unwrap(), Some("003_c".into()));
    }

    #[test]
    fn embedded_initial_migration_applies() {
        let mut conn = Connection::open_in_memory().unwrap();
        apply_embedded(&mut conn).unwrap();
        let version = current_version(&conn).unwrap();
        assert_eq!(version, Some("001_initial".into()));
    }
}
