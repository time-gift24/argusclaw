-- Add unique constraint on agents.display_name for upsert-by-name semantics
-- Create unique index (SQLite best practice, allows rollback)
CREATE UNIQUE INDEX IF NOT EXISTS idx_agents_display_name_unique
ON agents(display_name);

-- Handle existing duplicates: keep the newest (highest id), delete older ones
DELETE FROM agents
WHERE id NOT IN (
    SELECT MAX(id) FROM agents GROUP BY display_name
);
