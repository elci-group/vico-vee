-- Backfill migration for artifact databases created before migration tracking.
-- Adding the column is a no-op when it already exists.
ALTER TABLE vee_artifacts ADD COLUMN blob_hash TEXT;
