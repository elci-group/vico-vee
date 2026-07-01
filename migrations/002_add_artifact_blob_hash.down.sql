-- SQLite does not support dropping a column directly. A future migration can
-- rebuild the table without blob_hash if needed; for now this is intentionally
-- a no-op.
SELECT 1;
