-- Add optional credential reference to llm_providers
-- NULL = static API key (existing behavior)
-- Non-NULL = token-based auth via the referenced credential

ALTER TABLE llm_providers ADD COLUMN credential_id INTEGER REFERENCES credentials(id) ON DELETE SET NULL;

CREATE INDEX IF NOT EXISTS idx_llm_providers_credential_id ON llm_providers(credential_id);
