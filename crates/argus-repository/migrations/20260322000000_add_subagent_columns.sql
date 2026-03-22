-- Add subagent columns to agents table
-- parent_agent_id: links subagent to its parent agent
-- agent_type: distinguishes standard agents from subagents

ALTER TABLE agents ADD COLUMN parent_agent_id INTEGER REFERENCES agents(id);
ALTER TABLE agents ADD COLUMN agent_type TEXT NOT NULL DEFAULT 'standard' CHECK(agent_type IN ('standard', 'subagent'));

-- Index for efficient subagent lookup
CREATE INDEX IF NOT EXISTS idx_agents_parent_agent_id ON agents(parent_agent_id);
