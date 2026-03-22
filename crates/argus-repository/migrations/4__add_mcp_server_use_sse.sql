-- Add explicit HTTP transport mode switch:
-- 0 = streamable HTTP (default), 1 = legacy SSE
ALTER TABLE mcp_servers
ADD COLUMN use_sse INTEGER NOT NULL DEFAULT 0;
