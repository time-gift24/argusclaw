-- Add model_override column to threads table for per-thread model selection.
-- This allows sessions to use a non-default model from the provider's model list.
-- Existing threads will have NULL (meaning "use provider's default_model").

ALTER TABLE threads ADD COLUMN model_override TEXT;
