-- Add project isolation columns.
-- Every artifact and checkpoint now belongs to a project. Existing rows are
-- backfilled into the `default` project so isolation remains opt-in for
-- deployments that do not use multi-tenancy.

ALTER TABLE vee_artifacts ADD COLUMN project_id TEXT NOT NULL DEFAULT 'default';
CREATE INDEX IF NOT EXISTS idx_artifact_project ON vee_artifacts(project_id);
CREATE INDEX IF NOT EXISTS idx_artifact_project_execution ON vee_artifacts(project_id, execution_id);

ALTER TABLE vee_checkpoints ADD COLUMN project_id TEXT NOT NULL DEFAULT 'default';
CREATE INDEX IF NOT EXISTS idx_checkpoint_project ON vee_checkpoints(project_id);
CREATE INDEX IF NOT EXISTS idx_checkpoint_project_execution ON vee_checkpoints(project_id, execution_id);
