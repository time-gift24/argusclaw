-- Update MCP Servers table to use standard MCP config format
-- Changes:
-- - Rename transport to server_type (values: 'stdio', 'http')
-- - Add url column for HTTP type
-- - Add headers column (JSON) for HTTP type
-- - Add args column (JSON) for Stdio type
-- - Rename auth_token/auth_nonce to auth_token_ciphertext/auth_token_nonce

-- First, create backup table
ALTER TABLE mcp_servers RENAME TO mcp_servers_backup;

-- Create new table with updated schema
CREATE TABLE IF NOT EXISTS mcp_servers (
    id           INTEGER PRIMARY KEY AUTOINCREMENT,
    name         TEXT NOT NULL UNIQUE,           -- "filesystem" (used in tool naming: mcp_{name}_{tool})
    display_name TEXT NOT NULL,                  -- "Filesystem MCP"
    server_type  TEXT NOT NULL CHECK (server_type IN ('stdio', 'http')),
    url          TEXT,                           -- for http, e.g., "https://mcp.example.com/sse"
    headers      TEXT,                            -- JSON for http, e.g., {"Authorization": "Bearer ..."}
    command      TEXT,                           -- for stdio, e.g., "npx"
    args         TEXT,                           -- JSON for stdio, e.g., ["-y", "@modelcontextprotocol/server-filesystem", "/path"]
    auth_token_ciphertext BLOB,                   -- AES-256-GCM encrypted
    auth_token_nonce     BLOB,
    enabled      INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    created_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at   TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Index for looking up by name
CREATE UNIQUE INDEX IF NOT EXISTS idx_mcp_servers_name ON mcp_servers(name);

-- Index for enabled servers (common query)
CREATE INDEX IF NOT EXISTS idx_mcp_servers_enabled ON mcp_servers(enabled);

-- Migrate data from old table
-- transport = 'stdio' -> server_type = 'stdio'
-- transport = 'sse' -> server_type = 'http'
INSERT INTO mcp_servers (id, name, display_name, server_type, command, url, auth_token_ciphertext, auth_token_nonce, enabled, created_at, updated_at)
SELECT
    id,
    name,
    display_name,
    CASE transport
        WHEN 'stdio' THEN 'stdio'
        WHEN 'sse' THEN 'http'
        ELSE 'http'  -- default to http for unknown values
    END,
    command,
    url,
    auth_token,
    auth_nonce,
    enabled,
    created_at,
    updated_at
FROM mcp_servers_backup;

-- Drop backup table
DROP TABLE mcp_servers_backup;
