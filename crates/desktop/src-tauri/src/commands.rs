//! Tauri Commands for frontend to backend communication.

use std::collections::HashMap;

use claw::{
    AgentError, AgentId, AgentRecord, AppContext, LlmProviderId, LlmProviderKind,
    LlmProviderRecord, LlmProviderSummary, SecretString,
};
use serde::{Deserialize, Serialize};
use tauri::State;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProviderKind {
    OpenAiCompatible,
}

impl From<ProviderKind> for LlmProviderKind {
    fn from(kind: ProviderKind) -> Self {
        match kind {
            ProviderKind::OpenAiCompatible => Self::OpenAiCompatible,
        }
    }
}

impl From<LlmProviderKind> for ProviderKind {
    fn from(kind: LlmProviderKind) -> Self {
        match kind {
            LlmProviderKind::OpenAiCompatible => Self::OpenAiCompatible,
        }
    }
}

/// Input type for creating/updating a provider (api_key as plain string).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInput {
    pub id: String,
    pub kind: ProviderKind,
    pub display_name: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub is_default: bool,
    pub extra_headers: HashMap<String, String>,
}

impl From<ProviderInput> for LlmProviderRecord {
    fn from(input: ProviderInput) -> Self {
        Self {
            id: LlmProviderId::new(input.id),
            kind: input.kind.into(),
            display_name: input.display_name,
            base_url: input.base_url,
            api_key: SecretString::new(input.api_key),
            model: input.model,
            is_default: input.is_default,
            extra_headers: input.extra_headers,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderSummary {
    pub id: String,
    pub kind: ProviderKind,
    pub display_name: String,
    pub base_url: String,
    pub model: String,
    pub is_default: bool,
    pub extra_headers: HashMap<String, String>,
}

impl From<LlmProviderSummary> for ProviderSummary {
    fn from(summary: LlmProviderSummary) -> Self {
        Self {
            id: summary.id.to_string(),
            kind: summary.kind.into(),
            display_name: summary.display_name,
            base_url: summary.base_url,
            model: summary.model,
            is_default: summary.is_default,
            extra_headers: summary.extra_headers,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderRecord {
    pub id: String,
    pub kind: ProviderKind,
    pub display_name: String,
    pub base_url: String,
    pub api_key: String,
    pub model: String,
    pub is_default: bool,
    pub extra_headers: HashMap<String, String>,
}

impl From<LlmProviderRecord> for ProviderRecord {
    fn from(record: LlmProviderRecord) -> Self {
        Self {
            id: record.id.to_string(),
            kind: record.kind.into(),
            display_name: record.display_name,
            base_url: record.base_url,
            api_key: record.api_key.expose_secret().to_string(),
            model: record.model,
            is_default: record.is_default,
            extra_headers: record.extra_headers,
        }
    }
}

fn map_provider_lookup_result(
    result: Result<LlmProviderRecord, AgentError>,
) -> Result<Option<ProviderRecord>, String> {
    match result {
        Ok(record) => Ok(Some(record.into())),
        Err(AgentError::ProviderNotFound { .. }) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

// === LLMProvider Commands ===

#[tauri::command]
pub async fn list_providers(
    ctx: State<'_, std::sync::Arc<AppContext>>,
) -> Result<Vec<ProviderSummary>, String> {
    let providers = ctx.list_providers().await.map_err(|e| e.to_string())?;
    Ok(providers.into_iter().map(Into::into).collect())
}

#[tauri::command]
pub async fn get_provider(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    id: String,
) -> Result<Option<ProviderRecord>, String> {
    let provider = ctx.get_provider_record(&LlmProviderId::new(id)).await;
    map_provider_lookup_result(provider)
}

#[tauri::command]
pub async fn upsert_provider(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    record: ProviderInput,
) -> Result<(), String> {
    ctx.upsert_provider(record.into())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_provider(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    id: String,
) -> Result<bool, String> {
    ctx.delete_provider(&LlmProviderId::new(id))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_default_provider(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    id: String,
) -> Result<(), String> {
    ctx.set_default_provider(&LlmProviderId::new(id))
        .await
        .map_err(|e| e.to_string())
}

// === Agent Commands ===

#[tauri::command]
pub async fn list_agent_templates(
    ctx: State<'_, std::sync::Arc<AppContext>>,
) -> Result<Vec<AgentRecord>, String> {
    ctx.list_templates().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_agent_template(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    id: String,
) -> Result<Option<AgentRecord>, String> {
    ctx.get_template(&AgentId::new(id))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn upsert_agent_template(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    record: AgentRecord,
) -> Result<(), String> {
    ctx.upsert_template(record).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_agent_template(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    id: String,
) -> Result<bool, String> {
    ctx.delete_template(&AgentId::new(id))
        .await
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::{map_provider_lookup_result, ProviderInput, ProviderKind, ProviderRecord};
    use claw::{AgentError, LlmProviderId, LlmProviderKind, LlmProviderRecord, SecretString};

    #[test]
    fn provider_input_converts_into_domain_record() {
        let record: LlmProviderRecord = ProviderInput {
            id: "openai".to_string(),
            kind: ProviderKind::OpenAiCompatible,
            display_name: "OpenAI".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: "sk-test".to_string(),
            model: "gpt-4.1".to_string(),
            is_default: true,
            extra_headers: HashMap::new(),
        }
        .into();

        assert_eq!(record.id, LlmProviderId::new("openai"));
        assert_eq!(record.kind, LlmProviderKind::OpenAiCompatible);
        assert_eq!(record.api_key.expose_secret(), "sk-test");
    }

    #[test]
    fn provider_lookup_returns_none_for_missing_provider() {
        let result = map_provider_lookup_result(Err(AgentError::ProviderNotFound {
            id: "missing".to_string(),
        }))
        .expect("missing providers should map to None");

        assert!(result.is_none());
    }

    #[test]
    fn provider_record_conversion_exposes_plain_api_key() {
        let record = LlmProviderRecord {
            id: LlmProviderId::new("openai"),
            kind: LlmProviderKind::OpenAiCompatible,
            display_name: "OpenAI".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: SecretString::new("sk-test"),
            model: "gpt-4.1".to_string(),
            is_default: true,
            extra_headers: HashMap::new(),
        };

        let output = ProviderRecord::from(record);

        assert_eq!(output.id, "openai");
        assert_eq!(output.kind, ProviderKind::OpenAiCompatible);
        assert_eq!(output.api_key, "sk-test");
    }
}
