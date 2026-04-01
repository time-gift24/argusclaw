-- MCP server configuration and discovery cache
CREATE TABLE IF NOT EXISTS mcp_servers (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    display_name TEXT NOT NULL,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    transport_json TEXT NOT NULL,
    timeout_ms INTEGER NOT NULL DEFAULT 30000,
    status TEXT NOT NULL DEFAULT 'disabled',
    last_checked_at TEXT,
    last_success_at TEXT,
    last_error TEXT,
    discovered_tool_count INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS mcp_server_tools (
    server_id INTEGER NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    tool_name_original TEXT NOT NULL,
    description TEXT NOT NULL,
    schema_json TEXT NOT NULL,
    annotations_json TEXT,
    PRIMARY KEY (server_id, tool_name_original)
);

CREATE INDEX IF NOT EXISTS idx_mcp_server_tools_server_id
    ON mcp_server_tools(server_id);

CREATE TABLE IF NOT EXISTS agent_mcp_servers (
    agent_id INTEGER NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    server_id INTEGER NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    use_tool_whitelist INTEGER NOT NULL DEFAULT 0 CHECK (use_tool_whitelist IN (0, 1)),
    PRIMARY KEY (agent_id, server_id)
);

CREATE TABLE IF NOT EXISTS agent_mcp_tools (
    agent_id INTEGER NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
    server_id INTEGER NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
    tool_name_original TEXT NOT NULL,
    PRIMARY KEY (agent_id, server_id, tool_name_original)
);
