//! Configuration types for dev tools.

use argus_protocol::llm::{LlmProviderId, LlmProviderKind, ProviderSecretStatus, SecretString};
use serde::Deserialize;
use std::collections::HashMap;

/// TOML format for importing providers.
#[derive(Debug, Deserialize)]
pub struct ProviderImportFile {
    pub providers: Vec<ProviderEntry>,
}

#[derive(Debug, Deserialize)]
pub struct ProviderEntry {
    pub id: i64,
    pub kind: LlmProviderKind,
    pub display_name: String,
    pub base_url: String,
    pub api_key: String,
    pub models: Vec<String>,
    pub default_model: String,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub extra_headers: HashMap<String, String>,
}

impl ProviderImportFile {
    /// Convert into provider records.
    pub fn into_records(self) -> Vec<argus_protocol::llm::LlmProviderRecord> {
        self.providers
            .into_iter()
            .map(|p| argus_protocol::llm::LlmProviderRecord {
                id: LlmProviderId::new(p.id),
                kind: p.kind,
                display_name: p.display_name,
                base_url: p.base_url,
                api_key: SecretString::new(p.api_key),
                models: p.models,
                default_model: p.default_model,
                is_default: p.is_default,
                extra_headers: p.extra_headers,
                secret_status: ProviderSecretStatus::Ready,
            })
            .collect()
    }
}
