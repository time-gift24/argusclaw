PRAGMA foreign_keys = OFF;

CREATE TABLE agents_new (
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

INSERT INTO agents_new (
    id,
    display_name,
    description,
    version,
    provider_id,
    system_prompt,
    tool_names,
    max_tokens,
    temperature,
    created_at,
    updated_at
)
SELECT
    id,
    display_name,
    description,
    version,
    NULLIF(provider_id, ''),
    system_prompt,
    tool_names,
    max_tokens,
    temperature,
    created_at,
    updated_at
FROM agents;

DROP TABLE agents;
ALTER TABLE agents_new RENAME TO agents;

CREATE INDEX IF NOT EXISTS idx_agents_provider_id ON agents(provider_id);

PRAGMA foreign_keys = ON;
