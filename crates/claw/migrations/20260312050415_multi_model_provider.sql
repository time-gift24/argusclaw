-- Migration: Multi-model provider support
-- Replaces single 'model' field with 'models' array and 'default_model'

-- Step 1: Create new table with updated schema
CREATE TABLE IF NOT EXISTS llm_providers_new (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    display_name TEXT NOT NULL,
    base_url TEXT NOT NULL,
    models TEXT NOT NULL DEFAULT '[]',
    default_model TEXT NOT NULL,
    encrypted_api_key BLOB NOT NULL,
    api_key_nonce BLOB NOT NULL,
    extra_headers TEXT NOT NULL DEFAULT '{}',
    is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
);

-- Step 2: Migrate existing data
INSERT INTO llm_providers_new (
    id, kind, display_name, base_url, models, default_model,
    encrypted_api_key, api_key_nonce, extra_headers, is_default,
    created_at, updated_at
)
SELECT
    id, kind, display_name, base_url,
    json_array(model) AS models,
    model AS default_model,
    encrypted_api_key, api_key_nonce, extra_headers, is_default,
    created_at, updated_at
FROM llm_providers;

-- Step 3: Drop old table
DROP TABLE llm_providers;

-- Step 4: Rename new table
ALTER TABLE llm_providers_new RENAME TO llm_providers;

-- Step 5: Recreate indexes
CREATE UNIQUE INDEX IF NOT EXISTS idx_llm_providers_single_default
ON llm_providers (is_default)
WHERE is_default = 1;
