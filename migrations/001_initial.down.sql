-- Revert the initial schema.
--
-- Drops all tables created by migration 001.  The `vee_migrations` tracking
-- table is managed separately by the runner and is not touched here.

DROP INDEX IF EXISTS idx_artifact_execution;
DROP TABLE IF EXISTS vee_artifacts;

DROP INDEX IF EXISTS idx_ckpt_exec;
DROP TABLE IF EXISTS vee_checkpoints;

DROP TABLE IF EXISTS patterns;
