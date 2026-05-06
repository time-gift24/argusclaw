CREATE TABLE IF NOT EXISTS users (
    id UUID PRIMARY KEY,
    external_id TEXT NOT NULL UNIQUE,
    display_name TEXT,
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::TEXT),
    updated_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::TEXT)
);

CREATE TABLE IF NOT EXISTS accounts (
    id BIGINT PRIMARY KEY CHECK (id = 1),
    username TEXT NOT NULL,
    password BYTEA NOT NULL,
    nonce BYTEA NOT NULL,
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::TEXT),
    updated_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::TEXT)
);

CREATE TABLE IF NOT EXISTS llm_providers (
    id BIGSERIAL PRIMARY KEY,
    kind TEXT NOT NULL,
    display_name TEXT NOT NULL,
    base_url TEXT NOT NULL,
    models TEXT NOT NULL DEFAULT '[]',
    model_config TEXT NOT NULL DEFAULT '{}',
    default_model TEXT NOT NULL,
    encrypted_api_key BYTEA NOT NULL,
    api_key_nonce BYTEA NOT NULL,
    extra_headers TEXT NOT NULL DEFAULT '{}',
    meta_data TEXT NOT NULL DEFAULT '{}',
    is_default BOOLEAN NOT NULL DEFAULT FALSE,
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::TEXT),
    updated_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::TEXT)
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_llm_providers_single_default ON llm_providers (is_default) WHERE is_default = TRUE;

CREATE TABLE IF NOT EXISTS agents (
    id BIGSERIAL PRIMARY KEY,
    display_name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    version TEXT NOT NULL DEFAULT '1.0.0',
    provider_id BIGINT REFERENCES llm_providers(id) ON DELETE RESTRICT,
    model_id TEXT,
    system_prompt TEXT NOT NULL,
    tool_names TEXT NOT NULL DEFAULT '[]',
    subagent_names TEXT NOT NULL DEFAULT '[]',
    max_tokens BIGINT,
    temperature BIGINT,
    thinking_config TEXT,
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::TEXT),
    updated_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::TEXT)
);

CREATE TABLE IF NOT EXISTS sessions (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::TEXT),
    updated_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::TEXT)
);
CREATE INDEX IF NOT EXISTS idx_sessions_user_updated_at ON sessions(user_id, updated_at DESC);

CREATE TABLE IF NOT EXISTS threads (
    id UUID PRIMARY KEY,
    provider_id BIGINT NOT NULL REFERENCES llm_providers(id) ON DELETE RESTRICT,
    title TEXT,
    token_count BIGINT NOT NULL DEFAULT 0,
    turn_count BIGINT NOT NULL DEFAULT 0,
    session_id UUID REFERENCES sessions(id) ON DELETE CASCADE,
    template_id BIGINT REFERENCES agents(id),
    model_override TEXT,
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::TEXT),
    updated_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::TEXT)
);
CREATE INDEX IF NOT EXISTS idx_threads_session_id ON threads(session_id);
CREATE INDEX IF NOT EXISTS idx_threads_updated_at ON threads(updated_at DESC);

CREATE TABLE IF NOT EXISTS messages (
    id BIGSERIAL PRIMARY KEY,
    thread_id UUID NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    seq BIGINT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    tool_call_id TEXT,
    tool_name TEXT,
    tool_calls TEXT,
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::TEXT)
);
CREATE INDEX IF NOT EXISTS idx_messages_thread_seq ON messages(thread_id, seq);

CREATE TABLE IF NOT EXISTS jobs (
    id TEXT PRIMARY KEY NOT NULL,
    job_type TEXT NOT NULL DEFAULT 'standalone',
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    agent_id BIGINT NOT NULL REFERENCES agents(id) ON DELETE RESTRICT,
    context TEXT,
    prompt TEXT NOT NULL,
    thread_id UUID,
    group_id TEXT,
    depends_on TEXT NOT NULL DEFAULT '[]',
    cron_expr TEXT,
    scheduled_at TEXT,
    started_at TEXT,
    finished_at TEXT,
    parent_job_id TEXT REFERENCES jobs(id),
    result TEXT,
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::TEXT),
    updated_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::TEXT)
);

CREATE TABLE IF NOT EXISTS mcp_servers (
    id BIGSERIAL PRIMARY KEY,
    display_name TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    transport TEXT NOT NULL,
    timeout_ms BIGINT NOT NULL,
    status TEXT NOT NULL,
    last_checked_at TEXT,
    last_success_at TEXT,
    last_error TEXT,
    discovered_tool_count BIGINT NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::TEXT),
    updated_at TEXT NOT NULL DEFAULT (CURRENT_TIMESTAMP::TEXT)
);

CREATE TABLE IF NOT EXISTS mcp_discovered_tools (
    server_id BIGINT NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    tool_name_original TEXT NOT NULL,
    description TEXT NOT NULL,
    schema_json TEXT NOT NULL,
    annotations_json TEXT,
    PRIMARY KEY(server_id, tool_name_original)
);

CREATE TABLE IF NOT EXISTS agent_mcp_bindings (
    agent_id BIGINT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    server_id BIGINT NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    allowed_tools TEXT,
    PRIMARY KEY(agent_id, server_id)
);

CREATE TABLE IF NOT EXISTS agent_runs (
    id UUID PRIMARY KEY,
    agent_id BIGINT NOT NULL REFERENCES agents(id) ON DELETE RESTRICT,
    session_id UUID NOT NULL,
    thread_id UUID NOT NULL,
    prompt TEXT NOT NULL,
    status TEXT NOT NULL,
    result TEXT,
    error TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    completed_at TEXT
);

INSERT INTO llm_providers (kind, display_name, base_url, models, model_config, default_model, encrypted_api_key, api_key_nonce, extra_headers, meta_data, is_default)
VALUES ('openai-compatible', 'My LLM Provider', 'https://placeholder.example.com/v1', '["gpt-4o-mini"]', '{"gpt-4o-mini":{"max_context_window":128000}}', 'gpt-4o-mini', decode('', 'hex'), decode('', 'hex'), '{}', '{"account_token_source":"true"}', TRUE)
ON CONFLICT DO NOTHING;
