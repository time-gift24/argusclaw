-- Make provider_id nullable to allow agents without a fixed provider (like ArgusAgent)
-- SQLite doesn't support ALTER TABLE DROP CONSTRAINT, so we recreate the table

-- Create new agents table without FK constraint on provider_id
CREATE TABLE IF NOT EXISTS agents_new (
    id TEXT PRIMARY KEY,
    display_name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    version TEXT NOT NULL DEFAULT '1.0.0',
    provider_id TEXT,  -- Nullable, no FK constraint - allows default provider at runtime
    system_prompt TEXT NOT NULL,
    tool_names TEXT NOT NULL DEFAULT '[]',
    max_tokens INTEGER,
    temperature INTEGER,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Copy data from old table
INSERT INTO agents_new (id, display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature, created_at, updated_at)
SELECT id, display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature, created_at, updated_at
FROM agents;

-- Drop old table
DROP TABLE agents;

-- Rename new table
ALTER TABLE agents_new RENAME TO agents;

-- Recreate index (without FK-related index)
CREATE INDEX IF NOT EXISTS idx_agents_provider_id ON agents(provider_id);

-- Insert ArgusAgent - the default assistant for ArgusClaw
INSERT INTO agents (id, display_name, description, version, provider_id, system_prompt, tool_names)
VALUES (
    'argus',
    'Argus',
    'ArgusClaw 默认助手 - 帮助用户熟悉系统、创建新 Agent、直接对话',
    '1.0.0',
    NULL,
    '你是 Argus，ArgusClaw 的默认助手。你的职责是：
1. 直接与用户对话，解答问题
2. 帮助用户熟悉 ArgusClaw 系统
3. 协助用户创建和配置新的 Agent

你友好、专业、乐于助人。在帮助用户创建 Agent 时，你会询问他们的需求并提供合适的建议。',
    '[]'
);
