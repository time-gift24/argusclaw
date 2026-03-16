//! Tauri Commands for frontend to backend communication.

use std::collections::HashMap;

use claw::{
    AgentError, AgentId, AgentRecord, AppContext, DbError, LlmModelId, LlmModelRecord,
    LlmProviderId, LlmProviderKind, LlmProviderRecord, LlmProviderSummary, ProviderSecretStatus,
    ProviderTestResult, SecretString,
};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::subscription::ThreadSubscriptions;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProviderKind {
    #[serde(rename = "openai-compatible", alias = "open-ai-compatible")]
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
            is_default: input.is_default,
            extra_headers: input.extra_headers,
            secret_status: ProviderSecretStatus::Ready,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderSummary {
    pub id: String,
    pub kind: ProviderKind,
    pub display_name: String,
    pub base_url: String,
    pub is_default: bool,
    pub extra_headers: HashMap<String, String>,
    pub secret_status: ProviderSecretStatus,
}

impl From<LlmProviderSummary> for ProviderSummary {
    fn from(summary: LlmProviderSummary) -> Self {
        Self {
            id: summary.id.to_string(),
            kind: summary.kind.into(),
            display_name: summary.display_name,
            base_url: summary.base_url,
            is_default: summary.is_default,
            extra_headers: summary.extra_headers,
            secret_status: summary.secret_status,
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
    pub is_default: bool,
    pub extra_headers: HashMap<String, String>,
    pub secret_status: ProviderSecretStatus,
}

impl From<LlmProviderRecord> for ProviderRecord {
    fn from(record: LlmProviderRecord) -> Self {
        Self {
            id: record.id.to_string(),
            kind: record.kind.into(),
            display_name: record.display_name,
            base_url: record.base_url,
            api_key: record.api_key.expose_secret().to_string(),
            is_default: record.is_default,
            extra_headers: record.extra_headers,
            secret_status: record.secret_status,
        }
    }
}

fn build_provider_reentry_record(summary: LlmProviderSummary) -> ProviderRecord {
    ProviderRecord {
        id: summary.id.to_string(),
        kind: summary.kind.into(),
        display_name: summary.display_name,
        base_url: summary.base_url,
        api_key: String::new(),
        is_default: summary.is_default,
        extra_headers: summary.extra_headers,
        secret_status: summary.secret_status,
    }
}

// === Model Types ===

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelInput {
    pub id: String,
    pub provider_id: String,
    pub name: String,
    pub is_default: bool,
}

impl From<ModelInput> for LlmModelRecord {
    fn from(input: ModelInput) -> Self {
        Self {
            id: LlmModelId::new(input.id),
            provider_id: LlmProviderId::new(input.provider_id),
            name: input.name,
            is_default: input.is_default,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelRecord {
    pub id: String,
    pub provider_id: String,
    pub name: String,
    pub is_default: bool,
}

impl From<LlmModelRecord> for ModelRecord {
    fn from(record: LlmModelRecord) -> Self {
        Self {
            id: record.id.to_string(),
            provider_id: record.provider_id.to_string(),
            name: record.name,
            is_default: record.is_default,
        }
    }
}

#[cfg(test)]
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
    let provider_id = LlmProviderId::new(id);
    match ctx.get_provider_record(&provider_id).await {
        Ok(record) => Ok(Some(record.into())),
        Err(AgentError::ProviderNotFound { .. }) => Ok(None),
        Err(AgentError::Database(DbError::SecretDecryptionFailed { .. })) => {
            let summary = ctx
                .get_provider_summary(&provider_id)
                .await
                .map_err(|e| e.to_string())?;
            Ok(Some(build_provider_reentry_record(summary)))
        }
        Err(error) => Err(error.to_string()),
    }
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

#[tauri::command]
pub async fn test_provider_connection(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    id: String,
) -> Result<ProviderTestResult, String> {
    ctx.test_provider_connection(&LlmProviderId::new(id))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn test_provider_input(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    record: ProviderInput,
    model_name: String,
) -> Result<ProviderTestResult, String> {
    ctx.test_provider_record(record.into(), &model_name)
        .await
        .map_err(|e| e.to_string())
}

// === Model Commands ===

#[tauri::command]
pub async fn list_models_by_provider(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    provider_id: String,
) -> Result<Vec<ModelRecord>, String> {
    ctx.list_models_by_provider(&LlmProviderId::new(provider_id))
        .await
        .map(|models| models.into_iter().map(Into::into).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn upsert_model(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    record: ModelInput,
) -> Result<(), String> {
    ctx.upsert_model(record.into())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_model(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    id: String,
) -> Result<bool, String> {
    ctx.delete_model(&LlmModelId::new(id))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_default_model(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    id: String,
) -> Result<(), String> {
    ctx.set_default_model(&LlmModelId::new(id))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_builtin_tools(ctx: State<'_, std::sync::Arc<AppContext>>) -> Vec<String> {
    ctx.list_builtin_tool_names()
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

#[tauri::command]
pub async fn get_default_agent_template(
    ctx: State<'_, std::sync::Arc<AppContext>>,
) -> Result<AgentRecord, String> {
    ctx.get_default_agent_template()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_default_agent(
    ctx: State<'_, std::sync::Arc<AppContext>>,
) -> Result<String, String> {
    let agent_id = ctx
        .create_default_agent()
        .await
        .map_err(|e| e.to_string())?;
    Ok(agent_id.to_string())
}

// ========== User Auth Commands ==========

#[tauri::command]
pub async fn get_current_user(
    ctx: State<'_, std::sync::Arc<AppContext>>,
) -> Result<Option<claw::UserInfo>, String> {
    ctx.user()
        .get_current_user()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn has_any_user(ctx: State<'_, std::sync::Arc<AppContext>>) -> Result<bool, String> {
    ctx.user().has_any_user().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn setup_account(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    username: String,
    password: String,
) -> Result<(), String> {
    ctx.user()
        .setup_account(&username, &password)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn login(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    username: String,
    password: String,
) -> Result<claw::UserInfo, String> {
    ctx.user()
        .login(&username, &password)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn logout(ctx: State<'_, std::sync::Arc<AppContext>>) -> Result<(), String> {
    ctx.user().logout().await.map_err(|e| e.to_string())
}

// ========== Chat Session Commands ==========

/// Payload returned when creating a chat session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSessionPayload {
    /// Unique session key (template_id::provider_preference_id).
    pub session_key: String,
    /// The template ID this session was created from.
    pub template_id: String,
    /// The runtime agent ID for this session.
    pub runtime_agent_id: String,
    /// The thread ID for this session.
    pub thread_id: String,
    /// The effective provider ID bound to this session.
    pub effective_provider_id: String,
}

#[tauri::command]
pub async fn create_chat_session(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    subscriptions: State<'_, ThreadSubscriptions>,
    app: tauri::AppHandle,
    template_id: String,
    provider_preference_id: Option<String>,
) -> Result<ChatSessionPayload, String> {
    let provider_override = provider_preference_id
        .as_ref()
        .map(|id| LlmProviderId::new(id.clone()));

    let runtime = ctx
        .create_runtime_agent_from_template(
            &AgentId::new(template_id.clone()),
            provider_override.as_ref(),
        )
        .await
        .map_err(|e| e.to_string())?;

    let thread_id = ctx
        .create_thread(&runtime.runtime_agent_id, claw::ThreadConfig::default())
        .map_err(|e| e.to_string())?;

    let session_key = format!(
        "{}::{}",
        template_id,
        provider_preference_id.as_deref().unwrap_or("__default__")
    );

    // Start event forwarder
    subscriptions
        .start_forwarder(
            session_key.clone(),
            runtime.runtime_agent_id.clone(),
            thread_id,
            app,
            ctx.inner().clone(),
        )
        .await
        .map_err(|e| e.to_string())?;

    Ok(ChatSessionPayload {
        session_key,
        template_id,
        runtime_agent_id: runtime.runtime_agent_id.to_string(),
        thread_id: thread_id.to_string(),
        effective_provider_id: runtime.effective_provider_id.to_string(),
    })
}

#[tauri::command]
pub async fn send_message(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    runtime_agent_id: String,
    thread_id: String,
    content: String,
) -> Result<(), String> {
    let thread_id = claw::ThreadId::parse(&thread_id).map_err(|e| e.to_string())?;
    ctx.send_message(&AgentId::new(runtime_agent_id), &thread_id, content)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_thread_snapshot(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    runtime_agent_id: String,
    thread_id: String,
) -> Result<claw::ThreadSnapshot, String> {
    let thread_id = claw::ThreadId::parse(&thread_id).map_err(|e| e.to_string())?;
    ctx.get_thread_snapshot(&AgentId::new(runtime_agent_id), &thread_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn resolve_approval(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    runtime_agent_id: String,
    request_id: String,
    decision: String,
    resolved_by: Option<String>,
) -> Result<(), String> {
    let request_id = Uuid::parse_str(&request_id).map_err(|e| e.to_string())?;
    let decision = match decision.as_str() {
        "approved" => claw::ApprovalDecision::Approved,
        "denied" => claw::ApprovalDecision::Denied,
        _ => return Err(format!("Invalid approval decision: {}", decision)),
    };

    ctx.resolve_approval(
        &AgentId::new(runtime_agent_id),
        request_id,
        decision,
        resolved_by,
    )
    .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;

    use super::{
        build_provider_reentry_record, map_provider_lookup_result, ChatSessionPayload,
        ProviderInput, ProviderKind, ProviderRecord, ProviderSummary,
    };
    use claw::{
        AgentError, LlmProviderId, LlmProviderKind, LlmProviderRecord, LlmProviderSummary,
        ProviderSecretStatus, ProviderTestResult, ProviderTestStatus, SecretString,
    };

    #[test]
    fn provider_input_converts_into_domain_record() {
        let record: LlmProviderRecord = ProviderInput {
            id: "openai".to_string(),
            kind: ProviderKind::OpenAiCompatible,
            display_name: "OpenAI".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: "sk-test".to_string(),
            is_default: true,
            extra_headers: HashMap::new(),
        }
        .into();

        assert_eq!(record.id, LlmProviderId::new("openai"));
        assert_eq!(record.kind, LlmProviderKind::OpenAiCompatible);
        assert_eq!(record.api_key.expose_secret(), "sk-test");
        assert_eq!(record.secret_status, ProviderSecretStatus::Ready);
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
            is_default: true,
            extra_headers: HashMap::new(),
            secret_status: ProviderSecretStatus::Ready,
        };

        let output = ProviderRecord::from(record);

        assert_eq!(output.id, "openai");
        assert_eq!(output.kind, ProviderKind::OpenAiCompatible);
        assert_eq!(output.api_key, "sk-test");
        assert_eq!(output.secret_status, ProviderSecretStatus::Ready);
    }

    #[test]
    fn provider_summary_can_build_a_reentry_record_with_a_blank_api_key() {
        let record = build_provider_reentry_record(LlmProviderSummary {
            id: LlmProviderId::new("legacy"),
            kind: LlmProviderKind::OpenAiCompatible,
            display_name: "Legacy".to_string(),
            base_url: "https://legacy.example.com/v1".to_string(),
            model: "gpt-4.1".to_string(),
            is_default: false,
            extra_headers: HashMap::new(),
            secret_status: ProviderSecretStatus::RequiresReentry,
        });

        assert_eq!(record.id, "legacy");
        assert_eq!(record.api_key, "");
        assert_eq!(record.secret_status, ProviderSecretStatus::RequiresReentry);
    }

    #[test]
    fn provider_summary_conversion_exposes_secret_status_for_frontend() {
        let output = ProviderSummary::from(LlmProviderSummary {
            id: LlmProviderId::new("openai"),
            kind: LlmProviderKind::OpenAiCompatible,
            display_name: "OpenAI".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4.1".to_string(),
            is_default: true,
            extra_headers: HashMap::new(),
            secret_status: ProviderSecretStatus::Ready,
        });

        assert_eq!(output.id, "openai");
        assert_eq!(output.secret_status, ProviderSecretStatus::Ready);
    }

    #[test]
    fn provider_kind_serde_matches_frontend_payloads() {
        let parsed: ProviderKind = serde_json::from_value(json!("openai-compatible"))
            .expect("frontend value should parse");
        assert_eq!(parsed, ProviderKind::OpenAiCompatible);

        let serialized =
            serde_json::to_value(ProviderKind::OpenAiCompatible).expect("kind should serialize");
        assert_eq!(serialized, json!("openai-compatible"));
    }

    #[test]
    fn provider_test_result_serializes_for_frontend_consumption() {
        let result: ProviderTestResult = serde_json::from_value(json!({
            "provider_id": "openai",
            "model": "gpt-4.1",
            "base_url": "https://api.example.com/v1",
            "checked_at": "2026-03-16T12:00:00Z",
            "latency_ms": 57,
            "status": ProviderTestStatus::Success,
            "message": "Provider connection test succeeded.",
        }))
        .expect("result should deserialize");

        let serialized = serde_json::to_value(result).expect("result should serialize");

        assert_eq!(
            serialized,
            json!({
                "provider_id": "openai",
                "model": "gpt-4.1",
                "base_url": "https://api.example.com/v1",
                "checked_at": "2026-03-16T12:00:00Z",
                "latency_ms": 57,
                "status": "success",
                "message": "Provider connection test succeeded.",
            })
        );
    }

    #[test]
    fn chat_session_payload_serializes_effective_provider_id() {
        let payload = ChatSessionPayload {
            session_key: "arguswing::__default__".to_string(),
            template_id: "arguswing".to_string(),
            runtime_agent_id: "arguswing--runtime".to_string(),
            thread_id: claw::ThreadId::new().to_string(),
            effective_provider_id: "openai".to_string(),
        };

        let value = serde_json::to_value(payload).expect("payload should serialize");
        assert_eq!(value["effective_provider_id"], json!("openai"));
        assert_eq!(value["session_key"], json!("arguswing::__default__"));
    }
}
