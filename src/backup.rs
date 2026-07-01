//! Minimal backup/restore helpers.
//!
//! This module is intentionally lightweight while the backup/restore
//! workstream is in progress. It provides stub admin handlers so the server
//! module can compile.

use axum::{
    extract::State,
    response::{IntoResponse, Response},
    Json,
};

/// `POST /admin/backup` — create a backup tarball of the data directory.
pub async fn admin_backup(State(_state): State<crate::server::AppState>) -> Response {
    (
        axum::http::StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "success": false,
            "error": "backup endpoint not yet implemented",
        })),
    )
        .into_response()
}

/// `POST /admin/restore` — restore the data directory from a backup tarball.
pub async fn admin_restore(State(_state): State<crate::server::AppState>) -> Response {
    (
        axum::http::StatusCode::NOT_IMPLEMENTED,
        Json(serde_json::json!({
            "success": false,
            "error": "restore endpoint not yet implemented",
        })),
    )
        .into_response()
}
