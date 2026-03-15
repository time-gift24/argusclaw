use std::collections::HashMap;

use serde::Deserialize;

use claw::{DbError, LlmProviderId, LlmProviderKind, LlmProviderRecord, SecretString};

#[derive(Debug, Deserialize)]
pub struct ProviderImportFile {
    pub providers: Vec<ProviderImportRecord>,
}

#[derive(Debug, Deserialize)]
pub struct ProviderImportRecord {
    pub id: String,
    pub display_name: String,
    pub kind: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub extra_headers: HashMap<String, String>,
}

impl TryFrom<ProviderImportRecord> for LlmProviderRecord {
    type Error = DbError;

    fn try_from(value: ProviderImportRecord) -> Result<Self, Self::Error> {
        Ok(Self {
            id: LlmProviderId::new(value.id),
            kind: value.kind.parse::<LlmProviderKind>()?,
            display_name: value.display_name,
            base_url: value.base_url,
            api_key: SecretString::new(value.api_key),
            model: value.model,
            is_default: value.is_default,
            extra_headers: value.extra_headers,
        })
    }
}

impl ProviderImportFile {
    pub fn into_records(self) -> Result<Vec<LlmProviderRecord>, DbError> {
        self.providers.into_iter().map(TryInto::try_into).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::ProviderImportFile;

    #[test]
    fn parses_provider_import_toml() {
        let config: ProviderImportFile = toml::from_str(
            r#"
            [[providers]]
            id = "openai"
            display_name = "OpenAI"
            kind = "openai-compatible"
            base_url = "https://api.openai.com/v1"
            api_key = "sk-openai"
            model = "gpt-4o-mini"
            is_default = true
            "#,
        )
        .expect("provider import toml should parse");

        assert_eq!(config.providers.len(), 1);
        assert_eq!(config.providers[0].id, "openai");
        assert!(config.providers[0].is_default);
    }

    #[test]
    fn parses_provider_with_extra_headers() {
        let config: ProviderImportFile = toml::from_str(
            r#"
            [[providers]]
            id = "deepseek"
            display_name = "DeepSeek"
            kind = "openai-compatible"
            base_url = "https://api.deepseek.com/v1"
            api_key = "sk-xxx"
            model = "deepseek-chat"

            [providers.extra_headers]
            x-provider = "deepseek"
            x-custom = "value"
            "#,
        )
        .expect("provider import toml with headers should parse");

        assert_eq!(config.providers.len(), 1);
        let headers = &config.providers[0].extra_headers;
        assert_eq!(headers.get("x-provider"), Some(&"deepseek".to_string()));
        assert_eq!(headers.get("x-custom"), Some(&"value".to_string()));
    }
}
