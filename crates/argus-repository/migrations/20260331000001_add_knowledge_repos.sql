-- Knowledge repositories (repo = git URL, workspace = scenario tag)
CREATE TABLE IF NOT EXISTS knowledge_repos (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    repo TEXT NOT NULL UNIQUE,
    workspace TEXT NOT NULL
);

-- Agent <-> Workspace binding (many-to-many)
CREATE TABLE IF NOT EXISTS agent_knowledge_workspaces (
    agent_id INTEGER NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    workspace TEXT NOT NULL,
    PRIMARY KEY (agent_id, workspace)
);
