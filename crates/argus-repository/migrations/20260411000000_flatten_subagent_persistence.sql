-- Flatten subagent persistence: replace parent_agent_id/agent_type with subagent_names.

CREATE TABLE agents_new (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    display_name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    version TEXT NOT NULL DEFAULT '1.0.0',
    provider_id INTEGER REFERENCES llm_providers(id) ON DELETE RESTRICT,
    model_id TEXT,
    system_prompt TEXT NOT NULL,
    tool_names TEXT NOT NULL DEFAULT '[]',
    subagent_names TEXT NOT NULL DEFAULT '[]',
    max_tokens INTEGER,
    temperature INTEGER,
    thinking_config TEXT,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

INSERT INTO agents_new (
    id,
    display_name,
    description,
    version,
    provider_id,
    model_id,
    system_prompt,
    tool_names,
    subagent_names,
    max_tokens,
    temperature,
    thinking_config,
    created_at,
    updated_at
)
SELECT
    parent.id,
    parent.display_name,
    parent.description,
    parent.version,
    parent.provider_id,
    parent.model_id,
    parent.system_prompt,
    parent.tool_names,
    COALESCE(
        (
            SELECT json_group_array(children.display_name)
            FROM (
                SELECT child.display_name
                FROM agents child
                WHERE child.parent_agent_id = parent.id
                ORDER BY child.display_name
            ) children
        ),
        '[]'
    ),
    parent.max_tokens,
    parent.temperature,
    parent.thinking_config,
    parent.created_at,
    parent.updated_at
FROM agents parent;

DROP TABLE agents;
ALTER TABLE agents_new RENAME TO agents;

CREATE INDEX IF NOT EXISTS idx_agents_provider_id ON agents(provider_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_agents_display_name_unique ON agents(display_name);
