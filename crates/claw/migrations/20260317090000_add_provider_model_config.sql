ALTER TABLE llm_providers
ADD COLUMN model_config TEXT NOT NULL DEFAULT '{}';
