-- Consolidation of all migrations into a single file
-- This file combines all previous migrations in dependency order

-- ============================================================
-- 1. BASE SCHEMA (from 20260317080035_init.sql)
-- ============================================================

-- LLM Providers (INTEGER 自增 ID)
CREATE TABLE IF NOT EXISTS llm_providers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    kind TEXT NOT NULL,
    display_name TEXT NOT NULL,
    base_url TEXT NOT NULL,
    models TEXT NOT NULL DEFAULT '[]',
    default_model TEXT NOT NULL,
    encrypted_api_key BLOB NOT NULL,
    api_key_nonce BLOB NOT NULL,
    extra_headers TEXT NOT NULL DEFAULT '{}',
    is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_llm_providers_single_default
ON llm_providers (is_default) WHERE is_default = 1;

-- Agents (INTEGER 自增 ID)
CREATE TABLE IF NOT EXISTS agents (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    display_name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    version TEXT NOT NULL DEFAULT '1.0.0',
    provider_id INTEGER REFERENCES llm_providers(id) ON DELETE RESTRICT,
    system_prompt TEXT NOT NULL,
    tool_names TEXT NOT NULL DEFAULT '[]',
    max_tokens INTEGER,
    temperature INTEGER,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_agents_provider_id ON agents(provider_id);

-- Threads (保持 TEXT ID，provider_id 改为 INTEGER)
CREATE TABLE IF NOT EXISTS threads (
    id TEXT PRIMARY KEY,
    provider_id INTEGER NOT NULL REFERENCES llm_providers(id) ON DELETE RESTRICT,
    title TEXT,
    token_count INTEGER NOT NULL DEFAULT 0,
    turn_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_threads_provider_id ON threads(provider_id);
CREATE INDEX IF NOT EXISTS idx_threads_updated_at ON threads(updated_at DESC);

-- Messages (保持不变)
CREATE TABLE IF NOT EXISTS messages (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
    seq INTEGER NOT NULL,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    tool_call_id TEXT,
    tool_name TEXT,
    tool_calls TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_messages_thread_id ON messages(thread_id);
CREATE INDEX IF NOT EXISTS idx_messages_thread_seq ON messages(thread_id, seq);

-- Workflows (保持 TEXT ID)
CREATE TABLE IF NOT EXISTS workflows (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    status TEXT NOT NULL DEFAULT 'pending',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_workflows_status ON workflows(status);

-- Jobs (agent_id 改为 INTEGER)
CREATE TABLE IF NOT EXISTS jobs (
    id          TEXT PRIMARY KEY NOT NULL,
    job_type    TEXT NOT NULL DEFAULT 'standalone',
    name        TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'pending',
    agent_id    INTEGER NOT NULL REFERENCES agents(id) ON DELETE RESTRICT,
    context     TEXT,
    prompt      TEXT NOT NULL,
    thread_id   TEXT,
    group_id    TEXT,
    depends_on  TEXT NOT NULL DEFAULT '[]',
    cron_expr   TEXT,
    scheduled_at TEXT,
    started_at  TEXT,
    finished_at TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_jobs_status ON jobs(status);
CREATE INDEX IF NOT EXISTS idx_jobs_group_id ON jobs(group_id);
CREATE INDEX IF NOT EXISTS idx_jobs_agent_id ON jobs(agent_id);
CREATE INDEX IF NOT EXISTS idx_jobs_scheduled_at ON jobs(scheduled_at);
CREATE INDEX IF NOT EXISTS idx_jobs_job_type ON jobs(job_type);

-- Users (已是 INTEGER)
CREATE TABLE IF NOT EXISTS users (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    username TEXT UNIQUE NOT NULL,
    password_hash TEXT NOT NULL,
    password_salt TEXT NOT NULL,
    is_logged_in BOOLEAN NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================
-- 2. THINKING CONFIG (from 20260319160000_add_thinking_config.sql)
-- ============================================================

-- Add thinking_config column to agents table
ALTER TABLE agents ADD COLUMN thinking_config TEXT;

-- Set default value for existing records: disabled mode
UPDATE agents
SET thinking_config = '{"type":"disabled","clear_thinking":false}'
WHERE thinking_config IS NULL;

-- ============================================================
-- 3. UNIQUE AGENT NAME (from 20260320000000_add_agents_display_name_unique.sql)
-- ============================================================

-- Handle existing duplicates: keep the newest (highest id), delete older ones
DELETE FROM agents
WHERE id NOT IN (
    SELECT MAX(id) FROM agents GROUP BY display_name
);

-- Create unique index (SQLite best practice, allows rollback)
CREATE UNIQUE INDEX IF NOT EXISTS idx_agents_display_name_unique
ON agents(display_name);

-- ============================================================
-- 4. ACCOUNTS (from 20260320010000_add_accounts_and_credentials.sql)
-- ============================================================

-- Create accounts table (single-user)
CREATE TABLE IF NOT EXISTS accounts (
    id          INTEGER PRIMARY KEY CHECK (id = 1),
    username    TEXT NOT NULL,
    password    BLOB NOT NULL,
    nonce       BLOB NOT NULL,
    created_at  DATETIME DEFAULT CURRENT_TIMESTAMP,
    updated_at  DATETIME DEFAULT CURRENT_TIMESTAMP
);

-- ============================================================
-- 5. SESSIONS (from 20260317142753_add_sessions.sql)
-- ============================================================

-- Create sessions table
CREATE TABLE IF NOT EXISTS sessions (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    name TEXT NOT NULL,
    created_at TEXT NOT NULL DEFAULT (datetime('now')),
    updated_at TEXT NOT NULL DEFAULT (datetime('now'))
);

-- Index for listing sessions
CREATE INDEX IF NOT EXISTS idx_sessions_updated_at ON sessions(updated_at DESC);

-- Create default "Legacy" session for existing threads
INSERT INTO sessions (id, name, created_at, updated_at)
VALUES (1, 'Legacy', datetime('now'), datetime('now'));

-- Add session_id and template_id to threads (nullable initially)
ALTER TABLE threads ADD COLUMN session_id INTEGER REFERENCES sessions(id);
ALTER TABLE threads ADD COLUMN template_id INTEGER REFERENCES agents(id);

-- Update existing threads to belong to Legacy session and default template
UPDATE threads SET session_id = 1 WHERE session_id IS NULL;
UPDATE threads SET template_id = (SELECT id FROM agents ORDER BY id LIMIT 1) WHERE template_id IS NULL;

-- Add indexes for new columns
CREATE INDEX IF NOT EXISTS idx_threads_session_id ON threads(session_id);

-- ============================================================
-- 6. DEFAULT PROVIDER (from 20260320020000_add_default_provider.sql)
-- ============================================================

-- Insert a default provider with placeholder URL for user to configure
-- The API key is empty (will need to be set by user)
-- Includes a placeholder model name for initial testing
INSERT INTO llm_providers (kind, display_name, base_url, models, default_model, encrypted_api_key, api_key_nonce, extra_headers, is_default)
VALUES (
    'openai-compatible',
    'My LLM Provider',
    'https://placeholder.example.com/v1',
    '["gpt-4o-mini"]',
    'gpt-4o-mini',
    CAST(X'' AS BLOB),  -- empty encrypted api key
    CAST(X'' AS BLOB),   -- empty nonce
    '{}',
    1
);
