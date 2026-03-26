-- Add model_id column to agents table for per-agent default model override.
-- This allows agents to default to a specific model from their provider's model list.
-- Existing agents will have NULL (meaning "use provider's default_model").

ALTER TABLE agents ADD COLUMN model_id TEXT;
