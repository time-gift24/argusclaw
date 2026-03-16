-- New llm_models table
CREATE TABLE IF NOT EXISTS llm_models (
    id TEXT PRIMARY KEY,
    provider_id TEXT NOT NULL REFERENCES llm_providers(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_llm_models_provider_id ON llm_models(provider_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_llm_models_single_default_per_provider
    ON llm_models (provider_id, is_default) WHERE is_default = 1;

-- Migrate existing model data from providers into llm_models
INSERT INTO llm_models (id, provider_id, name, is_default, created_at, updated_at)
SELECT id || ':' || REPLACE(REPLACE(model, '/', '-'), ' ', '-'),
       id, model, 1, created_at, updated_at
FROM llm_providers WHERE model IS NOT NULL AND model != '';

-- Add model_id to agents (nullable for existing rows)
ALTER TABLE agents ADD COLUMN model_id TEXT REFERENCES llm_models(id) ON DELETE SET NULL;

-- Remove model from providers (SQLite 3.35+)
ALTER TABLE llm_providers DROP COLUMN model;
