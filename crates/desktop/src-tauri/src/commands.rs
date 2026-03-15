//! Tauri Commands for frontend to backend communication.

use std::collections::HashMap;

use claw::{
    AgentId, AgentRecord,
    LlmProviderId, LlmProviderKind, LlmProviderRecord, LlmProviderSummary, SecretString,
    AppContext,
};
use serde::{Deserialize, Serialize};
use tauri::State;

/// Input type for creating/updating a provider (api_key as plain string).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInput {
    pub id: String,
    pub kind: LlmProviderKind,
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
            kind: input.kind,
            display_name: input.display_name,
            base_url: input.base_url,
            api_key: SecretString::new(input.api_key),
            model: input.model,
            is_default: input.is_default,
            extra_headers: input.extra_headers,
        }
    }
}

// === LLMProvider Commands ===

#[tauri::command]
pub async fn list_providers(
    ctx: State<'_, std::sync::Arc<AppContext>>,
) -> Result<Vec<LlmProviderSummary>, String> {
    ctx.list_providers().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_provider(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    id: String,
) -> Result<Option<LlmProviderRecord>, String> {
    ctx.get_provider_record(&LlmProviderId::new(id))
        .await
        .map(Some)
        .or_else(|e| {
            if matches!(e, claw::AgentError::ProviderNotFound { .. }) {
                Ok(None)
            } else {
                Err(e.to_string())
            }
        })
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
    ctx.list_agent_templates()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_agent_template(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    id: String,
) -> Result<Option<AgentRecord>, String> {
    ctx.get_agent_template(&AgentId::new(id))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn upsert_agent_template(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    record: AgentRecord,
) -> Result<(), String> {
    ctx.upsert_agent_template(record)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_agent_template(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    id: String,
) -> Result<bool, String> {
    ctx.delete_agent_template(&AgentId::new(id))
        .await
        .map_err(|e| e.to_string())
}
