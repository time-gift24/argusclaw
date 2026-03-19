-- Insert a default provider with placeholder URL for user to configure
-- The API key is empty (will need to be set by user)
INSERT INTO llm_providers (kind, display_name, base_url, models, default_model, encrypted_api_key, api_key_nonce, extra_headers, is_default)
VALUES (
    'openai-compatible',
    'My LLM Provider',
    'https://placeholder.example.com/v1',
    '[]',
    '',
    X'',  -- empty encrypted api key
    X'',  -- empty nonce
    '{}',
    1
);
