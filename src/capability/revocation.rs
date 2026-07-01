use rusqlite::{params, Connection};
use std::collections::HashSet;
use std::path::Path;

pub const REVOCATION_DB_FILENAME: &str = "vee_capabilities.db";
pub const REVOCATION_TABLE: &str = "vee_revoked_capabilities";

pub fn load_revoked(db_path: &Path) -> Result<HashSet<String>, String> {
    let conn = open_revocation_db(db_path)?;
    let sql = format!("SELECT jti FROM {}", REVOCATION_TABLE);
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| format!("prepare load revoked: {}", e))?;
    let rows = stmt.query_map([], |row| row.get::<_, String>(0));
    let mut set = HashSet::new();
    if let Ok(rows) = rows {
        for row in rows.flatten() {
            set.insert(row);
        }
    }
    Ok(set)
}

pub fn persist_revocation(db_path: &Path, jti: &str) -> Result<(), String> {
    let conn = open_revocation_db(db_path)?;
    let sql = format!(
        "INSERT OR IGNORE INTO {} (jti) VALUES (?1)",
        REVOCATION_TABLE
    );
    conn.execute(&sql, params![jti])
        .map_err(|e| format!("insert revocation: {}", e))?;
    Ok(())
}

fn open_revocation_db(db_path: &Path) -> Result<Connection, String> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("create revocation db parent dir: {}", e))?;
    }
    let conn = Connection::open(db_path).map_err(|e| format!("open revocation db: {}", e))?;
    conn.execute(
        &format!(
            "CREATE TABLE IF NOT EXISTS {} (
                jti TEXT PRIMARY KEY,
                revoked_at TEXT NOT NULL DEFAULT (datetime('now'))
            )",
            REVOCATION_TABLE
        ),
        [],
    )
    .map_err(|e| format!("create revocation table: {}", e))?;
    Ok(conn)
}
