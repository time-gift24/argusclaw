//! Tauri Commands for frontend to backend communication.

use std::collections::HashMap;

use claw::{
    AgentError, AgentId, AgentRecord, AppContext, LlmProviderId, LlmProviderKind,
    LlmProviderRecord, ProviderSecretStatus, ProviderTestResult, SecretString,
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

/// Input type for creating/updating an agent (all IDs as i64).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInput {
    pub id: i64,
    pub display_name: String,
    pub description: String,
    pub version: String,
    pub provider_id: Option<i64>,
    pub system_prompt: String,
    pub tool_names: Vec<String>,
    pub max_tokens: Option<i64>,
    pub temperature: Option<f32>,
}

impl From<AgentInput> for AgentRecord {
    fn from(input: AgentInput) -> Self {
        Self {
            id: AgentId::new(input.id),
            display_name: input.display_name,
            description: input.description,
            version: input.version,
            provider_id: input.provider_id.map(LlmProviderId::new),
            system_prompt: input.system_prompt,
            tool_names: input.tool_names,
            max_tokens: input.max_tokens.map(|t| t as u32),
            temperature: input.temperature,
        }
    }
}

/// Input type for creating/updating a provider (api_key as plain string).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderInput {
    pub id: i64,
    pub kind: ProviderKind,
    pub display_name: String,
    pub base_url: String,
    pub api_key: String,
    pub models: Vec<String>,
    pub default_model: String,
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
            models: input.models,
            default_model: input.default_model,
            is_default: input.is_default,
            extra_headers: input.extra_headers,
            secret_status: ProviderSecretStatus::Ready,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderSummary {
    pub id: i64,
    pub kind: ProviderKind,
    pub display_name: String,
    pub base_url: String,
    pub models: Vec<String>,
    pub default_model: String,
    pub is_default: bool,
    pub extra_headers: HashMap<String, String>,
    pub secret_status: ProviderSecretStatus,
}

impl From<LlmProviderRecord> for ProviderSummary {
    fn from(record: LlmProviderRecord) -> Self {
        Self {
            id: record.id.into_inner(),
            kind: record.kind.into(),
            display_name: record.display_name,
            base_url: record.base_url,
            models: record.models,
            default_model: record.default_model,
            is_default: record.is_default,
            extra_headers: record.extra_headers,
            secret_status: record.secret_status,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderRecord {
    pub id: i64,
    pub kind: ProviderKind,
    pub display_name: String,
    pub base_url: String,
    pub api_key: String,
    pub models: Vec<String>,
    pub default_model: String,
    pub is_default: bool,
    pub extra_headers: HashMap<String, String>,
    pub secret_status: ProviderSecretStatus,
}

impl From<LlmProviderRecord> for ProviderRecord {
    fn from(record: LlmProviderRecord) -> Self {
        Self {
            id: record.id.into_inner(),
            kind: record.kind.into(),
            display_name: record.display_name,
            base_url: record.base_url,
            api_key: record.api_key.expose_secret().to_string(),
            models: record.models,
            default_model: record.default_model,
            is_default: record.is_default,
            extra_headers: record.extra_headers,
            secret_status: record.secret_status,
        }
    }
}

fn build_provider_reentry_record(record: LlmProviderRecord) -> ProviderRecord {
    ProviderRecord {
        id: record.id.into_inner(),
        kind: record.kind.into(),
        display_name: record.display_name,
        base_url: record.base_url,
        api_key: String::new(),
        models: record.models,
        default_model: record.default_model,
        is_default: record.is_default,
        extra_headers: record.extra_headers,
        secret_status: record.secret_status,
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
    id: i64,
) -> Result<Option<ProviderRecord>, String> {
    let provider_id = LlmProviderId::new(id);
    match ctx.get_provider_record(&provider_id).await {
        Ok(record) => {
            // If secret_status is RequiresReentry, build a re-entry record with blank api_key
            if record.secret_status == ProviderSecretStatus::RequiresReentry {
                Ok(Some(build_provider_reentry_record(record)))
            } else {
                Ok(Some(record.into()))
            }
        }
        Err(AgentError::ProviderNotFound { .. }) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

#[tauri::command]
pub async fn upsert_provider(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    record: ProviderInput,
) -> Result<String, String> {
    let record: LlmProviderRecord = record.into();
    let id = ctx
        .upsert_provider(record)
        .await
        .map_err(|e| e.to_string())?;
    Ok(id.to_string())
}

#[tauri::command]
pub async fn delete_provider(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    id: i64,
) -> Result<bool, String> {
    ctx.delete_provider(&LlmProviderId::new(id))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_default_provider(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    id: i64,
) -> Result<(), String> {
    ctx.set_default_provider(&LlmProviderId::new(id))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn test_provider_connection(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    id: i64,
    model: String,
) -> Result<ProviderTestResult, String> {
    ctx.test_provider_connection(&LlmProviderId::new(id), &model)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn test_provider_input(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    record: ProviderInput,
    model: String,
) -> Result<ProviderTestResult, String> {
    let record: LlmProviderRecord = record.into();
    ctx.test_provider_record(record, &model)
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
    id: i64,
) -> Result<Option<AgentRecord>, String> {
    ctx.get_template(&AgentId::new(id))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn upsert_agent_template(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    record: AgentInput,
) -> Result<(), String> {
    let record: AgentRecord = record.into();
    ctx.upsert_template(record).await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_agent_template(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    id: i64,
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
    /// The effective model being used for this session.
    pub effective_model: String,
}

#[tauri::command]
pub async fn create_chat_session(
    ctx: State<'_, std::sync::Arc<AppContext>>,
    subscriptions: State<'_, ThreadSubscriptions>,
    app: tauri::AppHandle,
    template_id: String,
    provider_preference_id: Option<String>,
    model_override: Option<String>,
) -> Result<ChatSessionPayload, String> {
    let template_id_i64: i64 = template_id
        .parse()
        .map_err(|e| format!("Invalid template id: {}", e))?;
    let provider_override = provider_preference_id
        .as_ref()
        .map(|id| {
            id.parse::<i64>()
                .map(LlmProviderId::new)
                .map_err(|e| format!("Invalid provider id: {}", e))
        })
        .transpose()?;

    let runtime = ctx
        .create_runtime_agent_from_template(
            &AgentId::new(template_id_i64),
            provider_override.as_ref(),
        )
        .await
        .map_err(|e| e.to_string())?;

    // Get provider record to determine effective model
    let provider_record = ctx
        .get_provider_record(&runtime.effective_provider_id)
        .await
        .map_err(|e| e.to_string())?;

    // Calculate effective model from override or provider's default
    let effective_model = model_override
        .as_ref()
        .filter(|m| !m.is_empty())
        .cloned()
        .unwrap_or_else(|| provider_record.default_model.clone());

    // Validate model is in provider's list
    if !provider_record.models.contains(&effective_model) {
        return Err(format!(
            "Model '{}' is not available in provider '{}'",
            effective_model, provider_record.id
        ));
    }

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
        effective_model,
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
    let runtime_agent_id: i64 = runtime_agent_id
        .parse()
        .map_err(|e| format!("Invalid agent id: {}", e))?;
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
    let runtime_agent_id: i64 = runtime_agent_id
        .parse()
        .map_err(|e| format!("Invalid agent id: {}", e))?;
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
    let runtime_agent_id: i64 = runtime_agent_id
        .parse()
        .map_err(|e| format!("Invalid agent id: {}", e))?;
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
        build_provider_reentry_record, ChatSessionPayload,
        ProviderInput, ProviderKind, ProviderRecord, ProviderSummary,
    };
    use claw::{
        LlmProviderId, LlmProviderKind, LlmProviderRecord,
        ProviderSecretStatus, ProviderTestResult, ProviderTestStatus, SecretString,
    };

    #[test]
    fn provider_input_converts_into_domain_record() {
        let record: LlmProviderRecord = ProviderInput {
            id: 1,
            kind: ProviderKind::OpenAiCompatible,
            display_name: "OpenAI".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: "sk-test".to_string(),
            models: vec!["gpt-4.1".to_string()],
            default_model: "gpt-4.1".to_string(),
            is_default: true,
            extra_headers: HashMap::new(),
        }
        .try_into()
        .expect("conversion should succeed");

        assert_eq!(record.id, LlmProviderId::new(1));
        assert_eq!(record.kind, LlmProviderKind::OpenAiCompatible);
        assert_eq!(record.api_key.expose_secret(), "sk-test");
        assert_eq!(record.secret_status, ProviderSecretStatus::Ready);
    }

    #[test]
    fn provider_record_conversion_exposes_plain_api_key() {
        let record = LlmProviderRecord {
            id: LlmProviderId::new(1),
            kind: LlmProviderKind::OpenAiCompatible,
            display_name: "OpenAI".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: SecretString::new("sk-test"),
            models: vec!["gpt-4.1".to_string()],
            default_model: "gpt-4.1".to_string(),
            is_default: true,
            extra_headers: HashMap::new(),
            secret_status: ProviderSecretStatus::Ready,
        };

        let output = ProviderRecord::from(record);

        assert_eq!(output.id, 1);
        assert_eq!(output.kind, ProviderKind::OpenAiCompatible);
        assert_eq!(output.api_key, "sk-test");
        assert_eq!(output.secret_status, ProviderSecretStatus::Ready);
    }

    #[test]
    fn provider_record_can_build_a_reentry_record_with_a_blank_api_key() {
        let record = build_provider_reentry_record(LlmProviderRecord {
            id: LlmProviderId::new(2),
            kind: LlmProviderKind::OpenAiCompatible,
            display_name: "Legacy".to_string(),
            base_url: "https://legacy.example.com/v1".to_string(),
            api_key: SecretString::new("sk-test"),
            models: vec!["gpt-4.1".to_string()],
            default_model: "gpt-4.1".to_string(),
            is_default: false,
            extra_headers: HashMap::new(),
            secret_status: ProviderSecretStatus::RequiresReentry,
        });

        assert_eq!(record.id, 2);
        assert_eq!(record.api_key, "");
        assert_eq!(record.secret_status, ProviderSecretStatus::RequiresReentry);
    }

    #[test]
    fn provider_record_conversion_exposes_secret_status_for_frontend() {
        let output = ProviderSummary::from(LlmProviderRecord {
            id: LlmProviderId::new(1),
            kind: LlmProviderKind::OpenAiCompatible,
            display_name: "OpenAI".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: SecretString::new("sk-test"),
            models: vec!["gpt-4.1".to_string()],
            default_model: "gpt-4.1".to_string(),
            is_default: true,
            extra_headers: HashMap::new(),
            secret_status: ProviderSecretStatus::Ready,
        });

        assert_eq!(output.id, 1);
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
            effective_model: "gpt-4.1".to_string(),
        };

        let value = serde_json::to_value(payload).expect("payload should serialize");
        assert_eq!(value["effective_provider_id"], json!("openai"));
        assert_eq!(value["effective_model"], json!("gpt-4.1"));
        assert_eq!(value["session_key"], json!("arguswing::__default__"));
    }
}
