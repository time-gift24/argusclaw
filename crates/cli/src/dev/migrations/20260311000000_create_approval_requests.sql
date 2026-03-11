CREATE TABLE IF NOT EXISTS approval_requests (
    id TEXT PRIMARY KEY NOT NULL,
    agent_id TEXT NOT NULL,
    tool_name TEXT NOT NULL,
    action TEXT NOT NULL,
    risk_level TEXT NOT NULL DEFAULT 'low',
    requested_at TEXT NOT NULL,
    timeout_secs INTEGER NOT NULL DEFAULT 60
);

CREATE INDEX IF NOT EXISTS idx_approval_requests_agent_id ON approval_requests(agent_id);
CREATE INDEX IF NOT EXISTS idx_approval_requests_tool_name ON approval_requests(tool_name);

CREATE TABLE IF NOT EXISTS approval_responses (
    request_id TEXT PRIMARY KEY NOT NULL,
    decision TEXT NOT NULL,
    decided_at TEXT NOT NULL,
    decided_by TEXT
);
