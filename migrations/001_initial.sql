-- Initial schema for the ViCo Execution Environment SQLite metadata stores.
-- This migration is run on every SQLite database used by vico-vee (artifacts,
-- checkpoints, and patterns). Tables that are not used by a particular database
-- are left empty but the schema version stays consistent across the service.

CREATE TABLE IF NOT EXISTS vee_artifacts (
    artifact_id TEXT PRIMARY KEY,
    execution_id TEXT NOT NULL,
    kind TEXT NOT NULL,
    metadata_json TEXT NOT NULL,
    blob_path TEXT NOT NULL,
    provenance_json TEXT,
    created_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_artifact_execution ON vee_artifacts(execution_id);

CREATE TABLE IF NOT EXISTS vee_checkpoints (
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
);

CREATE INDEX IF NOT EXISTS idx_ckpt_exec ON vee_checkpoints(execution_id);

CREATE TABLE IF NOT EXISTS patterns (
    pattern_id TEXT PRIMARY KEY,
    data TEXT NOT NULL
);
