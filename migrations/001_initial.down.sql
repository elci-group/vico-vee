-- Full down migration for the initial schema.
-- Drops all tables created by 001_initial.sql.
DROP TABLE IF EXISTS patterns;
DROP TABLE IF EXISTS vee_checkpoints;
DROP INDEX IF EXISTS idx_artifact_execution;
DROP TABLE IF EXISTS vee_artifacts;
