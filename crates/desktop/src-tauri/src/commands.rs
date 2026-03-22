//! Tauri Commands for frontend to backend communication.

use std::sync::Arc;

use argus_protocol::{
    AgentId, AgentRecord, ApprovalDecision, ChatMessage, LlmProviderId, LlmProviderRecord,
    LlmProviderRecordJson, ProviderId, ProviderSecretStatus, ProviderTestResult, Role,
    SecretString, SessionId, ThreadId,
};
use argus_tool::mcp::ConnectionTestResult;
use argus_wing::{ArgusWing, McpServerRecord};
use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::subscription::ThreadSubscriptions;

// ============================================================================
// LLM Provider Commands
// ============================================================================

#[tauri::command]
pub async fn list_providers(
    wing: State<'_, Arc<ArgusWing>>,
) -> Result<Vec<LlmProviderRecordJson>, String> {
    let providers = wing.list_providers().await.map_err(|e| e.to_string())?;
    Ok(providers.into_iter().map(Into::into).collect())
}

#[tauri::command]
pub async fn get_provider(
    wing: State<'_, Arc<ArgusWing>>,
    id: i64,
) -> Result<Option<LlmProviderRecordJson>, String> {
    let provider_id = LlmProviderId::new(id);
    match wing.get_provider_record(provider_id).await {
        Ok(record) => {
            // If secret_status is RequiresReentry, build a re-entry record with blank api_key
            if record.secret_status == ProviderSecretStatus::RequiresReentry {
                Ok(Some(build_provider_reentry_record(record)))
            } else {
                Ok(Some(record.into()))
            }
        }
        Err(argus_protocol::ArgusError::ProviderNotFound(_)) => Ok(None),
        Err(error) => Err(error.to_string()),
    }
}

fn build_provider_reentry_record(record: LlmProviderRecord) -> LlmProviderRecordJson {
    LlmProviderRecordJson {
        id: record.id.into_inner(),
        kind: record.kind,
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

#[tauri::command]
pub async fn upsert_provider(
    wing: State<'_, Arc<ArgusWing>>,
    record: LlmProviderRecordJson,
) -> Result<String, String> {
    let record = LlmProviderRecord {
        id: LlmProviderId::new(record.id),
        kind: record.kind,
        display_name: record.display_name,
        base_url: record.base_url,
        api_key: SecretString::new(record.api_key),
        models: record.models,
        default_model: record.default_model,
        is_default: record.is_default,
        extra_headers: record.extra_headers,
        secret_status: record.secret_status,
    };
    let id = wing
        .upsert_provider(record)
        .await
        .map_err(|e| e.to_string())?;
    Ok(id.to_string())
}

#[tauri::command]
pub async fn delete_provider(wing: State<'_, Arc<ArgusWing>>, id: i64) -> Result<bool, String> {
    wing.delete_provider(LlmProviderId::new(id))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_default_provider(wing: State<'_, Arc<ArgusWing>>, id: i64) -> Result<(), String> {
    wing.set_default_provider(LlmProviderId::new(id))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn test_provider_connection(
    wing: State<'_, Arc<ArgusWing>>,
    id: i64,
    model: String,
) -> Result<ProviderTestResult, String> {
    wing.test_provider_connection(LlmProviderId::new(id), &model)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn test_provider_input(
    wing: State<'_, Arc<ArgusWing>>,
    record: LlmProviderRecordJson,
    model: String,
) -> Result<ProviderTestResult, String> {
    let record = LlmProviderRecord {
        id: LlmProviderId::new(record.id),
        kind: record.kind,
        display_name: record.display_name,
        base_url: record.base_url,
        api_key: SecretString::new(record.api_key),
        models: record.models,
        default_model: record.default_model,
        is_default: record.is_default,
        extra_headers: record.extra_headers,
        secret_status: record.secret_status,
    };
    wing.test_provider_record(record, &model)
        .await
        .map_err(|e| e.to_string())
}

// ============================================================================
// Agent Template Commands
// ============================================================================

#[tauri::command]
pub async fn list_agent_templates(
    wing: State<'_, Arc<ArgusWing>>,
) -> Result<Vec<AgentRecord>, String> {
    wing.list_templates().await.map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_agent_template(
    wing: State<'_, Arc<ArgusWing>>,
    id: i64,
) -> Result<Option<AgentRecord>, String> {
    wing.get_template(AgentId::new(id))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn upsert_agent_template(
    wing: State<'_, Arc<ArgusWing>>,
    record: AgentRecord,
) -> Result<String, String> {
    let id = wing
        .upsert_template(record)
        .await
        .map_err(|e| e.to_string())?;
    Ok(id.to_string())
}

#[tauri::command]
pub async fn delete_agent_template(wing: State<'_, Arc<ArgusWing>>, id: i64) -> Result<(), String> {
    wing.delete_template(AgentId::new(id))
        .await
        .map_err(|e| e.to_string())
}

// ============================================================================
// Tool Commands
// ============================================================================

#[derive(Debug, Clone, serde::Serialize)]
pub struct ToolInfoPayload {
    pub name: String,
    pub description: String,
    pub risk_level: String,
    pub parameters: serde_json::Value,
}

#[tauri::command]
pub async fn list_tools(wing: State<'_, Arc<ArgusWing>>) -> Result<Vec<ToolInfoPayload>, String> {
    let tools = wing.list_tools().await;
    Ok(tools
        .into_iter()
        .map(|t| ToolInfoPayload {
            name: t.name,
            description: t.description,
            risk_level: format!("{:?}", t.risk_level).to_lowercase(),
            parameters: t.parameters,
        })
        .collect())
}

// ============================================================================
// Chat Session Commands
// ============================================================================

/// Payload returned when creating a chat session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatSessionPayload {
    /// Unique session key (template_id::provider_preference_id).
    pub session_key: String,
    /// The session ID for this chat.
    pub session_id: i64,
    /// The template ID this session was created from.
    pub template_id: i64,
    /// The thread ID for this session.
    pub thread_id: String,
    /// The effective provider ID bound to this session.
    /// `None` if no provider is configured (session will fail on first LLM call).
    pub effective_provider_id: Option<i64>,
}

/// Serialized message snapshot for frontend consumption.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessagePayload {
    pub role: Role,
    pub content: String,
    pub reasoning_content: Option<String>,
    pub tool_call_id: Option<String>,
    pub name: Option<String>,
    pub tool_calls: Option<Vec<argus_protocol::ToolCall>>,
}

impl From<&ChatMessage> for ChatMessagePayload {
    fn from(message: &ChatMessage) -> Self {
        Self {
            role: message.role,
            content: message.content.clone(),
            reasoning_content: message.reasoning_content.clone(),
            tool_call_id: message.tool_call_id.clone(),
            name: message.name.clone(),
            tool_calls: message.tool_calls.clone(),
        }
    }
}

/// Current snapshot of a chat thread.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadSnapshotPayload {
    pub session_id: i64,
    pub thread_id: String,
    pub messages: Vec<ChatMessagePayload>,
    pub turn_count: u32,
    pub token_count: u32,
    pub plan_item_count: usize,
}

#[tauri::command]
pub async fn create_chat_session(
    wing: State<'_, Arc<ArgusWing>>,
    subscriptions: State<'_, ThreadSubscriptions>,
    app: tauri::AppHandle,
    template_id: String,
    provider_preference_id: Option<String>,
) -> Result<ChatSessionPayload, String> {
    let template_id_i64: i64 = template_id
        .parse()
        .map_err(|e| format!("Invalid template id: {}", e))?;

    let provider_id = provider_preference_id
        .as_ref()
        .map(|id| {
            id.parse::<i64>()
                .map(ProviderId::new)
                .map_err(|e| format!("Invalid provider id: {}", e))
        })
        .transpose()?;

    // Create a new session for this chat
    let session_id = wing
        .create_session(&format!("Chat-{}", template_id))
        .await
        .map_err(|e| e.to_string())?;

    // Create thread with template and provider
    let thread_id = wing
        .create_thread(session_id, AgentId::new(template_id_i64), provider_id)
        .await
        .map_err(|e| e.to_string())?;

    // Get effective provider from the template or use the provided one
    let template = wing
        .get_template(AgentId::new(template_id_i64))
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| "Template not found".to_string())?;

    // Determine the effective provider ID:
    // 1. Use the explicitly provided provider preference
    // 2. Fall back to the template's configured provider
    // 3. Return None if no provider is configured (frontend should handle this case)
    let effective_provider_id = provider_id.or(template.provider_id).map(|p| p.inner());

    let session_key = format!(
        "{}::{}",
        template_id,
        provider_preference_id.as_deref().unwrap_or("__default__")
    );

    // Start event forwarder
    subscriptions
        .start_forwarder(
            session_key.clone(),
            session_id,
            thread_id,
            app,
            wing.inner().clone(),
        )
        .await
        .map_err(|e| e.to_string())?;

    Ok(ChatSessionPayload {
        session_key,
        session_id: session_id.inner(),
        template_id: template_id_i64,
        thread_id: thread_id.to_string(),
        effective_provider_id,
    })
}

#[tauri::command]
pub async fn send_message(
    wing: State<'_, Arc<ArgusWing>>,
    session_id: i64,
    thread_id: String,
    content: String,
) -> Result<(), String> {
    let thread_id = ThreadId::parse(&thread_id).map_err(|e| e.to_string())?;
    wing.send_message(SessionId::new(session_id), thread_id, content)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_thread_snapshot(
    wing: State<'_, Arc<ArgusWing>>,
    session_id: i64,
    thread_id: String,
) -> Result<ThreadSnapshotPayload, String> {
    let session_id = SessionId::new(session_id);
    let thread_id = ThreadId::parse(&thread_id).map_err(|e| e.to_string())?;

    let session = wing
        .load_session(session_id)
        .await
        .map_err(|e| e.to_string())?;
    let thread = session
        .get_thread(&thread_id)
        .ok_or_else(|| format!("Thread not found: {}", thread_id))?;

    let thread = thread.lock().await;

    Ok(ThreadSnapshotPayload {
        session_id: session_id.inner(),
        thread_id: thread_id.to_string(),
        messages: thread
            .history()
            .iter()
            .map(ChatMessagePayload::from)
            .collect(),
        turn_count: thread.turn_count(),
        token_count: thread.token_count(),
        plan_item_count: thread.info().plan_item_count,
    })
}

#[tauri::command]
pub fn resolve_approval(
    wing: State<'_, Arc<ArgusWing>>,
    request_id: String,
    decision: String,
    resolved_by: Option<String>,
) -> Result<(), String> {
    let request_id = Uuid::parse_str(&request_id).map_err(|e| e.to_string())?;
    let decision = match decision.as_str() {
        "approved" => ApprovalDecision::Approved,
        "denied" => ApprovalDecision::Denied,
        _ => return Err(format!("Invalid approval decision: {}", decision)),
    };

    wing.resolve_approval(request_id, decision, resolved_by)
        .map_err(|e| e.to_string())?;

    Ok(())
}

// ============================================================================
// Account Commands
// ============================================================================

#[derive(Debug, Clone, serde::Serialize)]
pub struct UserInfoPayload {
    pub username: String,
}

#[tauri::command]
pub async fn get_current_user(
    wing: State<'_, Arc<ArgusWing>>,
) -> Result<Option<UserInfoPayload>, String> {
    wing.account_manager()
        .get_current_user()
        .await
        .map(|opt| {
            opt.map(|u| UserInfoPayload {
                username: u.username,
            })
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn has_any_user(wing: State<'_, Arc<ArgusWing>>) -> Result<bool, String> {
    wing.account_manager()
        .has_account()
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn setup_account(
    wing: State<'_, Arc<ArgusWing>>,
    username: String,
    password: String,
) -> Result<(), String> {
    wing.account_manager()
        .setup_account(&username, &password)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn login(
    wing: State<'_, Arc<ArgusWing>>,
    username: String,
    password: String,
) -> Result<bool, String> {
    wing.account_manager()
        .login(&username, &password)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn logout(wing: State<'_, Arc<ArgusWing>>) -> Result<(), String> {
    wing.account_manager()
        .logout()
        .await
        .map_err(|e| e.to_string())
}

// ============================================================================
// Credential Commands
// ============================================================================

#[derive(Debug, Clone, serde::Serialize)]
pub struct CredentialSummaryPayload {
    pub id: i64,
    pub name: String,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct CredentialRecordPayload {
    pub id: i64,
    pub name: String,
    pub username: String,
    pub password: String,
}

#[tauri::command]
pub async fn list_credentials(
    wing: State<'_, Arc<ArgusWing>>,
) -> Result<Vec<CredentialSummaryPayload>, String> {
    wing.credential_store()
        .list()
        .await
        .map(|list| {
            list.into_iter()
                .map(|c| CredentialSummaryPayload {
                    id: c.id,
                    name: c.name,
                })
                .collect()
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_credential(
    wing: State<'_, Arc<ArgusWing>>,
    id: i64,
) -> Result<Option<CredentialRecordPayload>, String> {
    wing.credential_store()
        .get(id)
        .await
        .map(|opt| {
            opt.map(|c| CredentialRecordPayload {
                id: c.id,
                name: c.name,
                username: c.username,
                password: c.password,
            })
        })
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_credential(
    wing: State<'_, Arc<ArgusWing>>,
    name: String,
    username: String,
    password: String,
) -> Result<i64, String> {
    wing.credential_store()
        .add(&name, &username, &password)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_credential(
    wing: State<'_, Arc<ArgusWing>>,
    id: i64,
    username: Option<String>,
    password: Option<String>,
) -> Result<(), String> {
    wing.credential_store()
        .update(id, username.as_deref(), password.as_deref())
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_credential(wing: State<'_, Arc<ArgusWing>>, id: i64) -> Result<bool, String> {
    wing.credential_store()
        .delete(id)
        .await
        .map_err(|e| e.to_string())
}

// ============================================================================
// MCP Server Commands
// ============================================================================

/// Payload for MCP server used in frontend.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct McpServerPayload {
    pub id: i64,
    pub name: String,
    pub display_name: String,
    pub server_type: String,
    pub command: Option<String>,
    pub url: Option<String>,
    pub headers: Option<std::collections::HashMap<String, String>>,
    #[serde(default)]
    pub use_sse: bool,
    pub args: Option<Vec<String>>,
    pub enabled: bool,
}

impl From<McpServerRecord> for McpServerPayload {
    fn from(record: McpServerRecord) -> Self {
        Self {
            id: record.id.into_inner(),
            name: record.name,
            display_name: record.display_name,
            server_type: record.server_type.to_string(),
            command: record.command,
            url: record.url,
            headers: record.headers,
            use_sse: record.use_sse,
            args: record.args,
            enabled: record.enabled,
        }
    }
}

#[tauri::command]
pub async fn list_mcp_servers(
    wing: State<'_, Arc<ArgusWing>>,
) -> Result<Vec<McpServerPayload>, String> {
    wing.list_mcp_servers()
        .await
        .map(|servers| servers.into_iter().map(McpServerPayload::from).collect())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_mcp_server(
    wing: State<'_, Arc<ArgusWing>>,
    id: i64,
) -> Result<Option<McpServerPayload>, String> {
    use argus_wing::McpServerId;
    wing.get_mcp_server(McpServerId::new(id))
        .await
        .map(|opt| opt.map(McpServerPayload::from))
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn upsert_mcp_server(
    wing: State<'_, Arc<ArgusWing>>,
    payload: McpServerPayload,
) -> Result<i64, String> {
    use argus_wing::McpServerId;
    use argus_wing::ServerType;

    let server_type: ServerType = payload.server_type.parse().map_err(|e: String| e)?;

    let record = McpServerRecord {
        id: McpServerId::new(payload.id),
        name: payload.name,
        display_name: payload.display_name,
        server_type,
        command: payload.command,
        url: payload.url,
        headers: payload.headers,
        use_sse: payload.use_sse,
        args: payload.args,
        auth_token_ciphertext: None,
        auth_token_nonce: None,
        enabled: payload.enabled,
    };

    wing.upsert_mcp_server(record)
        .await
        .map(|id| id.into_inner())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_mcp_server(wing: State<'_, Arc<ArgusWing>>, id: i64) -> Result<bool, String> {
    use argus_wing::McpServerId;
    wing.delete_mcp_server(McpServerId::new(id))
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn test_mcp_server(
    wing: State<'_, Arc<ArgusWing>>,
    id: i64,
) -> Result<ConnectionTestResult, String> {
    use argus_wing::McpServerId;
    wing.test_mcp_server(McpServerId::new(id))
        .await
        .map_err(|e| e.to_string())
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use serde_json::json;

    use super::build_provider_reentry_record;
    use argus_protocol::{
        LlmProviderId, LlmProviderKind, LlmProviderRecord, ProviderSecretStatus,
        ProviderTestResult, ProviderTestStatus, SecretString, ThreadId,
    };

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
        use super::ChatSessionPayload;

        let payload = ChatSessionPayload {
            session_key: "arguswing::__default__".to_string(),
            session_id: 1,
            template_id: 1,
            thread_id: ThreadId::new().to_string(),
            effective_provider_id: Some(1),
        };

        let value = serde_json::to_value(payload).expect("payload should serialize");
        assert_eq!(value["effective_provider_id"], json!(1));
        assert_eq!(value["session_key"], json!("arguswing::__default__"));
        assert_eq!(value["session_id"], json!(1));
    }

    #[test]
    fn chat_session_payload_serializes_none_effective_provider_id() {
        use super::ChatSessionPayload;

        let payload = ChatSessionPayload {
            session_key: "arguswing::__default__".to_string(),
            session_id: 1,
            template_id: 1,
            thread_id: ThreadId::new().to_string(),
            effective_provider_id: None,
        };

        let value = serde_json::to_value(payload).expect("payload should serialize");
        assert_eq!(value["effective_provider_id"], json!(null));
    }
}
