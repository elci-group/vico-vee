DROP INDEX IF EXISTS idx_artifact_project_execution;
DROP INDEX IF EXISTS idx_artifact_project;
ALTER TABLE vee_artifacts DROP COLUMN project_id;

DROP INDEX IF EXISTS idx_checkpoint_project_execution;
DROP INDEX IF EXISTS idx_checkpoint_project;
ALTER TABLE vee_checkpoints DROP COLUMN project_id;
