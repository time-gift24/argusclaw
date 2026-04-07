-- PostgreSQL schema for the Argus server product.
-- Creates IF NOT EXISTS for all tables; safe to run repeatedly.

-- Users for OAuth2-based multi-user isolation.
CREATE TABLE IF NOT EXISTS users (
    id BIGSERIAL PRIMARY KEY,
    external_subject TEXT NOT NULL UNIQUE,
    account TEXT NOT NULL,
    display_name TEXT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Provider token-exchange credentials stored server-side.
CREATE TABLE IF NOT EXISTS provider_token_credentials (
    provider_id BIGINT NOT NULL PRIMARY KEY,
    username TEXT NOT NULL,
    ciphertext BYTEA NOT NULL,
    nonce BYTEA NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- LLM Providers
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
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Agents
CREATE TABLE IF NOT EXISTS agents (
    id BIGSERIAL PRIMARY KEY,
    display_name TEXT NOT NULL UNIQUE,
    description TEXT NOT NULL DEFAULT '',
    version TEXT NOT NULL DEFAULT '1.0.0',
    provider_id BIGINT REFERENCES llm_providers(id) ON DELETE RESTRICT,
    model_id TEXT,
    system_prompt TEXT NOT NULL,
    tool_names TEXT NOT NULL DEFAULT '[]',
    max_tokens BIGINT,
    temperature BIGINT,
    thinking_config TEXT,
    parent_agent_id BIGINT REFERENCES agents(id),
    agent_type TEXT NOT NULL DEFAULT 'standard' CHECK (agent_type IN ('standard', 'subagent')),
    is_enabled BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Sessions (TEXT UUID ID)
CREATE TABLE IF NOT EXISTS sessions (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    owner_user_id BIGINT REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Threads (TEXT UUID ID)
CREATE TABLE IF NOT EXISTS threads (
    id TEXT PRIMARY KEY,
    provider_id BIGINT NOT NULL REFERENCES llm_providers(id) ON DELETE RESTRICT,
    title TEXT,
    token_count BIGINT NOT NULL DEFAULT 0,
    turn_count BIGINT NOT NULL DEFAULT 0,
    session_id TEXT REFERENCES sessions(id),
    template_id BIGINT REFERENCES agents(id),
    model_override TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Messages
CREATE TABLE IF NOT EXISTS messages (
    id BIGSERIAL PRIMARY KEY,
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    seq BIGINT NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    tool_call_id TEXT,
    tool_name TEXT,
    tool_calls TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Jobs
CREATE TABLE IF NOT EXISTS jobs (
    id TEXT PRIMARY KEY NOT NULL,
    job_type TEXT NOT NULL DEFAULT 'standalone',
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    agent_id BIGINT NOT NULL REFERENCES agents(id) ON DELETE RESTRICT,
    context TEXT,
    prompt TEXT NOT NULL,
    thread_id TEXT,
    group_id TEXT,
    depends_on TEXT NOT NULL DEFAULT '[]',
    cron_expr TEXT,
    scheduled_at TEXT,
    started_at TEXT,
    finished_at TEXT,
    parent_job_id TEXT REFERENCES jobs(id),
    result TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- MCP tables
CREATE TABLE IF NOT EXISTS mcp_servers (
    id BIGSERIAL PRIMARY KEY,
    display_name TEXT NOT NULL,
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    transport_json TEXT NOT NULL,
    timeout_ms BIGINT NOT NULL DEFAULT 30000,
    status TEXT NOT NULL DEFAULT 'disabled',
    last_checked_at TEXT,
    last_success_at TEXT,
    last_error TEXT,
    discovered_tool_count BIGINT NOT NULL DEFAULT 0,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS mcp_server_tools (
    server_id BIGINT NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    tool_name_original TEXT NOT NULL,
    description TEXT NOT NULL,
    schema_json TEXT NOT NULL,
    annotations_json TEXT,
    PRIMARY KEY (server_id, tool_name_original)
);

CREATE TABLE IF NOT EXISTS agent_mcp_servers (
    agent_id BIGINT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    server_id BIGINT NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    use_tool_whitelist BOOLEAN NOT NULL DEFAULT FALSE,
    PRIMARY KEY (agent_id, server_id)
);

CREATE TABLE IF NOT EXISTS agent_mcp_tools (
    agent_id BIGINT NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    server_id BIGINT NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    tool_name_original TEXT NOT NULL,
    PRIMARY KEY (agent_id, server_id, tool_name_original)
);

-- Indexes
CREATE INDEX IF NOT EXISTS idx_sessions_owner_user_id ON sessions(owner_user_id);
CREATE INDEX IF NOT EXISTS idx_threads_session_id ON threads(session_id);
CREATE INDEX IF NOT EXISTS idx_threads_updated_at ON threads(updated_at DESC);
CREATE INDEX IF NOT EXISTS idx_messages_thread_id ON messages(thread_id);
CREATE INDEX IF NOT EXISTS idx_messages_thread_seq ON messages(thread_id, seq);
CREATE INDEX IF NOT EXISTS idx_jobs_status ON jobs(status);
CREATE INDEX IF NOT EXISTS idx_jobs_agent_id ON jobs(agent_id);
