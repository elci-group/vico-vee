//! Backup and restore for the vico-vee data directory.
//!
//! Provides admin-only HTTP handlers and CLI helpers to create and restore
//! timestamped tarballs of the persistent metadata database, artifact blobs,
//! capability keys, and revocations.

use axum::{
    body::Bytes,
    extract::State,
    http::{header, HeaderValue, StatusCode},
    response::{IntoResponse, Response},
    Json as JsonResponse,
};
use flate2::{read::GzDecoder, write::GzEncoder, Compression};
use std::path::{Path, PathBuf};
use tar::{Archive, Builder};

use crate::server::AppState;

/// Create a timestamped backup of `data_dir` at `output`.
///
/// If `output` is a directory, a file named `vico-vee-backup-<timestamp>.tar.gz`
/// is created inside it.
pub fn create_backup(data_dir: &Path, output: &Path) -> Result<PathBuf, String> {
    let dest = if output.is_dir() {
        output.join(format!(
            "vico-vee-backup-{}.tar.gz",
            chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
        ))
    } else {
        output.to_path_buf()
    };

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("create backup parent dir: {e}"))?;
    }

    let file = std::fs::File::create(&dest).map_err(|e| format!("create backup file: {e}"))?;
    let enc = GzEncoder::new(file, Compression::default());
    let mut tar = Builder::new(enc);

    let db_path = data_dir.join("vee_artifacts.db");
    let snapshot_path = data_dir.join(".vee_artifacts.db.snapshot");

    if db_path.exists() {
        snapshot_db(&db_path, &snapshot_path)?;
        tar.append_path_with_name(&snapshot_path, "vee_artifacts.db")
            .map_err(|e| format!("append database snapshot: {e}"))?;
        let _ = std::fs::remove_file(&snapshot_path);
    }

    let dir_entries: [(&str, PathBuf); 3] = [
        ("artifacts", data_dir.join("artifacts")),
        ("keys", data_dir.join("keys")),
        ("revocations", data_dir.join("revocations")),
    ];
    for (name, path) in dir_entries {
        if path.is_dir() {
            tar.append_dir_all(name, &path)
                .map_err(|e| format!("append dir {}: {e}", path.display()))?;
        }
    }

    let enc = tar
        .into_inner()
        .map_err(|e| format!("finish tar archive: {e}"))?;
    enc.finish()
        .map_err(|e| format!("finish gzip stream: {e}"))?;

    Ok(dest)
}

fn snapshot_db(src: &Path, dst: &Path) -> Result<(), String> {
    let src_conn = rusqlite::Connection::open(src).map_err(|e| format!("open source db: {e}"))?;
    let mut dst_conn =
        rusqlite::Connection::open(dst).map_err(|e| format!("open snapshot db: {e}"))?;
    let backup = rusqlite::backup::Backup::new(&src_conn, &mut dst_conn)
        .map_err(|e| format!("initialize sqlite backup: {e}"))?;
    backup
        .step(-1)
        .map_err(|e| format!("sqlite backup step: {e}"))?;
    Ok(())
}

/// Restore `data_dir` from a backup archive created by [`create_backup`].
pub fn restore_backup(data_dir: &Path, archive: &Path) -> Result<(), String> {
    std::fs::create_dir_all(data_dir).map_err(|e| format!("create data dir: {e}"))?;
    let file = std::fs::File::open(archive).map_err(|e| format!("open archive: {e}"))?;
    let dec = GzDecoder::new(file);
    let mut archive = Archive::new(dec);
    archive
        .unpack(data_dir)
        .map_err(|e| format!("unpack archive: {e}"))?;
    Ok(())
}

/// CLI entry point for the `backup` command.
pub fn run_backup(
    config: &crate::config::Config,
    output: Option<PathBuf>,
) -> Result<PathBuf, String> {
    let output_path = output.unwrap_or_else(|| {
        std::env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join(format!(
                "vico-vee-backup-{}.tar.gz",
                chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
            ))
    });
    create_backup(&config.data_dir, &output_path)
}

/// CLI entry point for the `restore` command.
pub fn run_restore(config: &crate::config::Config, input: PathBuf) -> Result<(), String> {
    restore_backup(&config.data_dir, &input)
}

/// `POST /admin/backup` — create a backup tarball.
///
/// Admin scope is enforced by the auth middleware applied to all routes.
pub async fn admin_backup(State(state): State<AppState>) -> Response {
    let data_dir = state.config.data_dir.clone();
    let filename = format!(
        "vico-vee-backup-{}.tar.gz",
        chrono::Utc::now().format("%Y%m%dT%H%M%SZ")
    );
    let tmp_dir = std::env::temp_dir().join(format!("vico-vee-backup-{}", uuid::Uuid::new_v4()));
    let tmp_path = tmp_dir.join(&filename);

    let result = tokio::task::spawn_blocking(move || create_backup(&data_dir, &tmp_path)).await;

    match result {
        Ok(Ok(path)) => match tokio::fs::read(&path).await {
            Ok(bytes) => {
                let _ = tokio::fs::remove_dir_all(&tmp_dir).await;
                let disposition = format!("attachment; filename=\"{}\"", filename);
                (
                    [
                        (
                            header::CONTENT_TYPE,
                            HeaderValue::from_static("application/octet-stream"),
                        ),
                        (
                            header::CONTENT_DISPOSITION,
                            HeaderValue::try_from(disposition)
                                .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
                        ),
                    ],
                    bytes,
                )
                    .into_response()
            }
            Err(e) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("read backup bytes: {e}"),
            )
                .into_response(),
        },
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("backup failed: {e}"),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("backup task failed: {e}"),
        )
            .into_response(),
    }
}

/// `POST /admin/restore` — restore from a backup tarball.
///
/// Admin scope is enforced by the auth middleware applied to all routes.
pub async fn admin_restore(State(state): State<AppState>, body: Bytes) -> Response {
    let tmp_path =
        std::env::temp_dir().join(format!("vico-vee-restore-{}.tar.gz", uuid::Uuid::new_v4()));
    if let Err(e) = tokio::fs::write(&tmp_path, &body).await {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("write uploaded archive: {e}"),
        )
            .into_response();
    }

    let data_dir = state.config.data_dir.clone();
    let tmp_path_for_task = tmp_path.clone();
    let result =
        tokio::task::spawn_blocking(move || restore_backup(&data_dir, &tmp_path_for_task)).await;

    let _ = tokio::fs::remove_file(&tmp_path).await;

    match result {
        Ok(Ok(())) => JsonResponse(serde_json::json!({
            "success": true,
            "message": "restore completed",
        }))
        .into_response(),
        Ok(Err(e)) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("restore failed: {e}"),
        )
            .into_response(),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("restore task failed: {e}"),
        )
            .into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::Config;
    use crate::server::{router, AppState};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    #[test]
    fn backup_restore_roundtrip() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();

        // Create a real SQLite database so the online snapshot succeeds.
        let db_path = data_dir.join("vee_artifacts.db");
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute("CREATE TABLE test (id INTEGER)", []).unwrap();
        }

        let artifacts_dir = data_dir.join("artifacts");
        std::fs::create_dir_all(&artifacts_dir).unwrap();
        std::fs::write(artifacts_dir.join("test.txt"), "hello artifacts").unwrap();

        let keys_dir = data_dir.join("keys");
        std::fs::create_dir_all(&keys_dir).unwrap();
        std::fs::write(keys_dir.join("key.pem"), "secret-key").unwrap();

        let backup_path = tmp.path().join("backup.tar.gz");
        create_backup(&data_dir, &backup_path).unwrap();

        // Mutate the data directory after the backup is taken.
        std::fs::write(artifacts_dir.join("test.txt"), "mutated").unwrap();
        std::fs::remove_file(keys_dir.join("key.pem")).unwrap();

        restore_backup(&data_dir, &backup_path).unwrap();

        assert_eq!(
            std::fs::read_to_string(artifacts_dir.join("test.txt")).unwrap(),
            "hello artifacts"
        );
        assert_eq!(
            std::fs::read_to_string(keys_dir.join("key.pem")).unwrap(),
            "secret-key"
        );

        // Verify the restored database is intact.
        let conn = rusqlite::Connection::open(&db_path).unwrap();
        conn.execute("SELECT id FROM test", []).unwrap();
    }

    #[tokio::test]
    async fn admin_backup_route_returns_tarball() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();

        let db_path = data_dir.join("vee_artifacts.db");
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute("CREATE TABLE t (id INTEGER)", []).unwrap();
        }
        std::fs::create_dir_all(data_dir.join("artifacts")).unwrap();
        std::fs::write(data_dir.join("artifacts").join("keep.txt"), "keep").unwrap();

        let config = Config {
            data_dir,
            ..Config::default()
        };
        let state = AppState::test_new(config);
        let app = router(state);

        let response = app
            .oneshot(
                Request::builder()
                    .uri("/admin/backup")
                    .method("POST")
                    .header("Authorization", "Bearer test-admin-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok()),
            Some("application/octet-stream")
        );
    }

    #[tokio::test]
    async fn admin_backup_requires_admin_scope() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();

        let config = Config {
            data_dir,
            ..Config::default()
        };
        let state = AppState::test_new(config);
        let app = router(state);

        // No auth header -> 401 (enforced by the shared auth middleware).
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/backup")
                    .method("POST")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);

        // Read-only token -> 403.
        let response = app
            .oneshot(
                Request::builder()
                    .uri("/admin/backup")
                    .method("POST")
                    .header("Authorization", "Bearer test-read-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn admin_restore_roundtrip_via_route() {
        let tmp = tempfile::tempdir().unwrap();
        let data_dir = tmp.path().join("data");
        std::fs::create_dir_all(&data_dir).unwrap();

        // Seed the data directory.
        let db_path = data_dir.join("vee_artifacts.db");
        {
            let conn = rusqlite::Connection::open(&db_path).unwrap();
            conn.execute("CREATE TABLE t (id INTEGER)", []).unwrap();
        }
        std::fs::create_dir_all(data_dir.join("artifacts")).unwrap();
        std::fs::write(data_dir.join("artifacts").join("keep.txt"), "keep").unwrap();

        let config = Config {
            data_dir: data_dir.clone(),
            ..Config::default()
        };
        let state = AppState::test_new(config);
        let app = router(state.clone());

        // Take a backup through the admin route.
        let backup_response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/admin/backup")
                    .method("POST")
                    .header("Authorization", "Bearer test-admin-token")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(backup_response.status(), StatusCode::OK);
        let backup_bytes = axum::body::to_bytes(backup_response.into_body(), usize::MAX)
            .await
            .unwrap();

        // Mutate and delete files.
        std::fs::write(data_dir.join("artifacts").join("keep.txt"), "mutated").unwrap();

        // Restore through the admin route.
        let restore_response = app
            .oneshot(
                Request::builder()
                    .uri("/admin/restore")
                    .method("POST")
                    .header("Authorization", "Bearer test-admin-token")
                    .body(Body::from(backup_bytes))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(restore_response.status(), StatusCode::OK);

        assert_eq!(
            std::fs::read_to_string(data_dir.join("artifacts").join("keep.txt")).unwrap(),
            "keep"
        );

        state.vee.stop().await;
    }
}
