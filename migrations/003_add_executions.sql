-- Persistent execution metadata table.
-- Stores the full ExecutionResult as JSON so the in-memory daemon state can be
-- rebuilt after a process restart.
CREATE TABLE IF NOT EXISTS vee_executions (
    execution_id TEXT PRIMARY KEY,
    project_id TEXT NOT NULL DEFAULT 'default',
    status TEXT NOT NULL,
    result_json TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

CREATE INDEX IF NOT EXISTS idx_exec_project ON vee_executions(project_id);
CREATE INDEX IF NOT EXISTS idx_exec_status ON vee_executions(status);
