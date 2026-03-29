-- Persistent workflow templates and execution metadata.

CREATE TABLE IF NOT EXISTS workflow_templates (
    id TEXT PRIMARY KEY NOT NULL,
    name TEXT NOT NULL,
    version INTEGER NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS workflow_template_nodes (
    template_id TEXT NOT NULL REFERENCES workflow_templates(id) ON DELETE CASCADE,
    node_key TEXT NOT NULL,
    name TEXT NOT NULL,
    agent_id INTEGER NOT NULL REFERENCES agents(id) ON DELETE RESTRICT,
    prompt TEXT NOT NULL,
    context TEXT,
    depends_on_keys TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (template_id, node_key)
);

ALTER TABLE workflows ADD COLUMN template_id TEXT REFERENCES workflow_templates(id);
ALTER TABLE workflows ADD COLUMN template_version INTEGER;
ALTER TABLE workflows ADD COLUMN initiating_thread_id TEXT REFERENCES threads(id);

ALTER TABLE jobs ADD COLUMN node_key TEXT;

CREATE INDEX IF NOT EXISTS idx_workflows_template_id ON workflows(template_id);
CREATE INDEX IF NOT EXISTS idx_workflows_initiating_thread_id ON workflows(initiating_thread_id);
CREATE INDEX IF NOT EXISTS idx_workflow_template_nodes_template_id ON workflow_template_nodes(template_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_jobs_group_id_node_key_unique ON jobs(group_id, node_key);
