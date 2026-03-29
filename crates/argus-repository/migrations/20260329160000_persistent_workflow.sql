-- Persistent workflow templates and execution metadata.

CREATE TABLE IF NOT EXISTS workflow_templates (
    id TEXT NOT NULL,
    version INTEGER NOT NULL,
    name TEXT NOT NULL,
    description TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (id, version)
);

CREATE TABLE IF NOT EXISTS workflow_template_nodes (
    template_id TEXT NOT NULL,
    template_version INTEGER NOT NULL,
    node_key TEXT NOT NULL,
    name TEXT NOT NULL,
    agent_id INTEGER NOT NULL REFERENCES agents(id) ON DELETE RESTRICT,
    prompt TEXT NOT NULL,
    context TEXT,
    depends_on_keys TEXT NOT NULL DEFAULT '[]',
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (template_id, template_version, node_key),
    FOREIGN KEY (template_id, template_version) REFERENCES workflow_templates(id, version) ON DELETE CASCADE
);

ALTER TABLE workflows ADD COLUMN template_id TEXT REFERENCES workflow_templates(id);
ALTER TABLE workflows ADD COLUMN template_version INTEGER;
ALTER TABLE workflows ADD COLUMN initiating_thread_id TEXT REFERENCES threads(id);

ALTER TABLE jobs ADD COLUMN node_key TEXT;

CREATE INDEX IF NOT EXISTS idx_workflows_template_id ON workflows(template_id);
CREATE INDEX IF NOT EXISTS idx_workflows_initiating_thread_id ON workflows(initiating_thread_id);
CREATE INDEX IF NOT EXISTS idx_workflow_template_nodes_template_id_version ON workflow_template_nodes(template_id, template_version);
CREATE UNIQUE INDEX IF NOT EXISTS idx_jobs_group_id_node_key_unique ON jobs(group_id, node_key);
