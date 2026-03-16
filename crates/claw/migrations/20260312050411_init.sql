-- LLM Providers
CREATE TABLE IF NOT EXISTS llm_providers (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    display_name TEXT NOT NULL,
    base_url TEXT NOT NULL,
    model TEXT NOT NULL,
    encrypted_api_key BLOB NOT NULL,
    api_key_nonce BLOB NOT NULL,
    extra_headers TEXT NOT NULL DEFAULT '{}',
    is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_llm_providers_single_default
ON llm_providers (is_default)
WHERE is_default = 1;

-- Agents
CREATE TABLE IF NOT EXISTS agents (
    id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    version TEXT NOT NULL DEFAULT '1.0.0',
    provider_id TEXT REFERENCES llm_providers(id) ON DELETE RESTRICT,
    system_prompt TEXT NOT NULL,
    tool_names TEXT NOT NULL DEFAULT '[]',
    max_tokens INTEGER,
    temperature INTEGER,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_agents_provider_id ON agents(provider_id);

-- Approval Requests
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

-- Workflows
CREATE TABLE IF NOT EXISTS workflows (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS stages (
    id TEXT PRIMARY KEY NOT NULL,
    workflow_id TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    sequence INTEGER NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS jobs (
    id TEXT PRIMARY KEY NOT NULL,
    stage_id TEXT NOT NULL REFERENCES stages(id) ON DELETE CASCADE,
    agent_id TEXT NOT NULL REFERENCES agents(id) ON DELETE RESTRICT,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    started_at TEXT,
    finished_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_stages_workflow_id ON stages(workflow_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_stages_workflow_sequence ON stages(workflow_id, sequence);
CREATE INDEX IF NOT EXISTS idx_workflows_status ON workflows(status);
CREATE INDEX IF NOT EXISTS idx_jobs_stage_id ON jobs(stage_id);
CREATE INDEX IF NOT EXISTS idx_jobs_agent_id ON jobs(agent_id);
