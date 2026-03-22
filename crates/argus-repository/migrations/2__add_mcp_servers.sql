-- MCP Servers table for storing MCP server configurations
CREATE TABLE IF NOT EXISTS mcp_servers (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    name         TEXT NOT NULL UNIQUE,           -- "filesystem" (used in tool naming: mcp_{name}_{tool})
    display_name TEXT NOT NULL,                    -- "Filesystem MCP"
    transport    TEXT NOT NULL CHECK (transport IN ('stdio', 'sse')),
    command      TEXT,                             -- for stdio, e.g., "npx -y @modelcontextprotocol/server-filesystem"
    url          TEXT,                             -- for sse, e.g., "https://mcp.example.com"
    auth_token   BLOB,                             -- AES-256-GCM encrypted
    auth_nonce   BLOB,
    enabled      INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    created_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Index for looking up by name
CREATE UNIQUE INDEX IF NOT EXISTS idx_mcp_servers_name ON mcp_servers(name);

-- Index for enabled servers (common query)
CREATE INDEX IF NOT EXISTS idx_mcp_servers_enabled ON mcp_servers(enabled);
