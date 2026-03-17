use std::collections::HashMap;

use serde::Deserialize;

use claw::{
    DbError, LlmProviderId, LlmProviderKind, LlmProviderRecord, ProviderSecretStatus, SecretString,
};

#[derive(Debug, Deserialize)]
pub struct ProviderImportFile {
    pub providers: Vec<ProviderImportRecord>,
}

#[derive(Debug, Deserialize)]
pub struct ProviderImportRecord {
    /// Display name for the provider (used as human-readable identifier).
    pub display_name: String,
    pub kind: String,
    pub base_url: String,
    pub api_key: String,
    /// Single model (for backward compatibility). If provided, this becomes the only model and default_model.
    #[serde(default)]
    pub model: Option<String>,
    /// List of models. Required if `model` is not provided.
    #[serde(default)]
    pub models: Vec<String>,
    /// Default model. Required if `models` is provided. Ignored if `model` is provided.
    #[serde(default)]
    pub default_model: Option<String>,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub extra_headers: HashMap<String, String>,
}

impl TryFrom<ProviderImportRecord> for LlmProviderRecord {
    type Error = DbError;

    fn try_from(value: ProviderImportRecord) -> Result<Self, Self::Error> {
        // Determine models and default_model based on whether single model or multi-model format is used
        let (models, default_model) = if let Some(model) = value.model {
            // Single model format (backward compatibility)
            (vec![model.clone()], model)
        } else if !value.models.is_empty() {
            // Multi-model format
            let default = value
                .default_model
                .clone()
                .unwrap_or_else(|| value.models[0].clone());
            if !value.models.contains(&default) {
                return Err(DbError::QueryFailed {
                    reason: format!(
                        "Default model '{}' must be in models list for provider '{}'",
                        default, value.display_name
                    ),
                });
            }
            (value.models, default)
        } else {
            return Err(DbError::QueryFailed {
                reason: format!(
                    "Provider '{}' must have either 'model' or 'models' field",
                    value.display_name
                ),
            });
        };

        // Use placeholder ID (0) - database will auto-generate the actual ID
        Ok(Self {
            id: LlmProviderId::new(0),
            kind: value.kind.parse::<LlmProviderKind>()?,
            display_name: value.display_name,
            base_url: value.base_url,
            api_key: SecretString::new(value.api_key),
            models,
            default_model,
            is_default: value.is_default,
            extra_headers: value.extra_headers,
            secret_status: ProviderSecretStatus::Ready,
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
        assert_eq!(config.providers[0].display_name, "OpenAI");
        assert!(config.providers[0].is_default);
    }

    #[test]
    fn parses_provider_with_extra_headers() {
        let config: ProviderImportFile = toml::from_str(
            r#"
            [[providers]]
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
