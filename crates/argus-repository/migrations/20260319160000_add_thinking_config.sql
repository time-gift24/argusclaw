-- Migration: Add thinking_config column to agents table
-- Created: 2025-03-19
-- Description: Adds support for configuring thinking/reasoning mode per agent

-- Add thinking_config column to agents table
ALTER TABLE agents ADD COLUMN thinking_config TEXT;

-- Set default value for existing records: disabled mode
UPDATE agents
SET thinking_config = '{"type":"disabled","clear_thinking":false}'
WHERE thinking_config IS NULL;
