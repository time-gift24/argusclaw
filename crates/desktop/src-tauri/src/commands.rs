//! Tauri Commands for frontend to backend communication.

use std::sync::Arc;

use argus_protocol::{
    AgentId, AgentMcpBinding, AgentMcpServerBinding, AgentRecord, ChatMessage, JobRuntimeState,
    LlmProviderId, LlmProviderRecord, LlmProviderRecordJson, McpDiscoveredToolRecord,
    McpServerRecord, McpServerStatus, ProviderId, ProviderSecretStatus, ProviderTestResult, Role,
    SecretString, SessionId, ThreadId, ThreadPoolSnapshot, ThreadPoolState,
};
use argus_wing::{
    ArgusWing, PendingAssistantTrace, PendingToolCallTrace, PendingToolStatus,
};
use serde::{Deserialize, Serialize};
use tauri::State;

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
        model_config: record.model_config,
        default_model: record.default_model,
        is_default: record.is_default,
        extra_headers: record.extra_headers,
        secret_status: record.secret_status,
        meta_data: record.meta_data,
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
        model_config: record.model_config,
        default_model: record.default_model,
        is_default: record.is_default,
        extra_headers: record.extra_headers,
        secret_status: record.secret_status,
        meta_data: record.meta_data,
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
        model_config: record.model_config,
        default_model: record.default_model,
        is_default: record.is_default,
        extra_headers: record.extra_headers,
        secret_status: record.secret_status,
        meta_data: record.meta_data,
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

#[tauri::command]
pub async fn get_thread_pool_snapshot(
    wing: State<'_, Arc<ArgusWing>>,
) -> Result<ThreadPoolSnapshot, String> {
    Ok(wing.thread_pool_snapshot())
}

#[tauri::command]
pub async fn get_thread_pool_state(
    wing: State<'_, Arc<ArgusWing>>,
) -> Result<ThreadPoolState, String> {
    Ok(wing.thread_pool_state())
}

#[tauri::command]
pub async fn get_job_runtime_state(
    wing: State<'_, Arc<ArgusWing>>,
) -> Result<JobRuntimeState, String> {
    Ok(wing.job_runtime_state())
}

// ============================================================================
// MCP Commands
// ============================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct McpConnectionTestResultPayload {
    pub status: McpServerStatus,
    pub checked_at: String,
    pub latency_ms: u64,
    pub discovered_tools: Vec<McpDiscoveredToolRecord>,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct AgentMcpBindingPayload {
    pub server_id: i64,
    pub allowed_tools: Option<Vec<String>>,
}

impl From<AgentMcpBinding> for AgentMcpBindingPayload {
    fn from(binding: AgentMcpBinding) -> Self {
        Self {
            server_id: binding.server.server_id,
            allowed_tools: binding.allowed_tools,
        }
    }
}

impl AgentMcpBindingPayload {
    fn into_binding(self, agent_id: AgentId) -> AgentMcpBinding {
        AgentMcpBinding {
            server: AgentMcpServerBinding {
                agent_id,
                server_id: self.server_id,
            },
            allowed_tools: self.allowed_tools,
        }
    }
}

#[tauri::command]
pub async fn list_mcp_servers(
    wing: State<'_, Arc<ArgusWing>>,
) -> Result<Vec<McpServerRecord>, String> {
    wing.list_mcp_servers()
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn get_mcp_server(
    wing: State<'_, Arc<ArgusWing>>,
    id: i64,
) -> Result<Option<McpServerRecord>, String> {
    wing.get_mcp_server(id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn upsert_mcp_server(
    wing: State<'_, Arc<ArgusWing>>,
    record: McpServerRecord,
) -> Result<i64, String> {
    wing.upsert_mcp_server(record)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn delete_mcp_server(wing: State<'_, Arc<ArgusWing>>, id: i64) -> Result<bool, String> {
    wing.delete_mcp_server(id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn test_mcp_server_input(
    wing: State<'_, Arc<ArgusWing>>,
    record: McpServerRecord,
) -> Result<McpConnectionTestResultPayload, String> {
    wing.test_mcp_server_input(record)
        .await
        .map(|result| McpConnectionTestResultPayload {
            status: result.status,
            checked_at: result.checked_at,
            latency_ms: result.latency_ms,
            discovered_tools: result.discovered_tools,
            message: result.message,
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn test_mcp_server_connection(
    wing: State<'_, Arc<ArgusWing>>,
    id: i64,
) -> Result<McpConnectionTestResultPayload, String> {
    wing.test_mcp_server_connection(id)
        .await
        .map(|result| McpConnectionTestResultPayload {
            status: result.status,
            checked_at: result.checked_at,
            latency_ms: result.latency_ms,
            discovered_tools: result.discovered_tools,
            message: result.message,
        })
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn list_mcp_server_tools(
    wing: State<'_, Arc<ArgusWing>>,
    server_id: i64,
) -> Result<Vec<McpDiscoveredToolRecord>, String> {
    wing.list_mcp_server_tools(server_id)
        .await
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn list_agent_mcp_bindings(
    wing: State<'_, Arc<ArgusWing>>,
    agent_id: i64,
) -> Result<Vec<AgentMcpBindingPayload>, String> {
    wing.list_agent_mcp_bindings(AgentId::new(agent_id))
        .await
        .map(|bindings| bindings.into_iter().map(Into::into).collect())
        .map_err(|error| error.to_string())
}

#[tauri::command]
pub async fn set_agent_mcp_bindings(
    wing: State<'_, Arc<ArgusWing>>,
    agent_id: i64,
    bindings: Vec<AgentMcpBindingPayload>,
) -> Result<(), String> {
    let agent_id = AgentId::new(agent_id);
    let bindings = bindings
        .into_iter()
        .map(|binding| binding.into_binding(agent_id))
        .collect::<Vec<_>>();
    wing.set_agent_mcp_bindings(agent_id, bindings)
        .await
        .map_err(|error| error.to_string())
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
    pub session_id: String,
    /// The template ID this session was created from.
    pub template_id: i64,
    /// The thread ID for this session.
    pub thread_id: String,
    /// The effective provider ID bound to this session.
    /// `None` if no provider is configured (session will fail on first LLM call).
    pub effective_provider_id: Option<i64>,
    /// The effective model currently bound to this session's thread.
    #[serde(default)]
    pub effective_model: Option<String>,
}

/// Session summary for listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummaryPayload {
    pub id: String,
    pub name: String,
    pub thread_count: i64,
    pub updated_at: String,
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
    pub metadata: Option<ChatMessageMetadataPayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
#[allow(clippy::enum_variant_names)]
pub enum ChatMessageMetadataModePayload {
    CompactionPrompt,
    CompactionSummary,
    CompactionReplay,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessageMetadataPayload {
    pub summary: bool,
    pub mode: Option<ChatMessageMetadataModePayload>,
    pub synthetic: bool,
    pub collapsed_by_default: bool,
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
            metadata: message
                .metadata
                .as_ref()
                .map(|metadata| ChatMessageMetadataPayload {
                    summary: metadata.summary,
                    mode: metadata.mode.map(|mode| match mode {
                        argus_protocol::llm::ChatMessageMetadataMode::CompactionPrompt => {
                            ChatMessageMetadataModePayload::CompactionPrompt
                        }
                        argus_protocol::llm::ChatMessageMetadataMode::CompactionSummary => {
                            ChatMessageMetadataModePayload::CompactionSummary
                        }
                        argus_protocol::llm::ChatMessageMetadataMode::CompactionReplay => {
                            ChatMessageMetadataModePayload::CompactionReplay
                        }
                    }),
                    synthetic: metadata.synthetic,
                    collapsed_by_default: metadata.collapsed_by_default,
                }),
        }
    }
}

/// Serialized in-flight assistant snapshot recovered from turn event traces.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingAssistantPayload {
    pub turn_number: u32,
    pub content: String,
    pub reasoning: String,
    pub tool_calls: Vec<PendingToolCallPayload>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingToolCallPayload {
    pub index: usize,
    pub call_id: Option<String>,
    pub name: Option<String>,
    pub arguments_text: String,
    pub status: PendingToolStatusPayload,
    pub arguments: Option<serde_json::Value>,
    pub result: Option<serde_json::Value>,
    pub is_error: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingToolStatusPayload {
    Pending,
    Started,
    Completed,
}

impl From<PendingAssistantTrace> for PendingAssistantPayload {
    fn from(pending: PendingAssistantTrace) -> Self {
        Self {
            turn_number: pending.turn_number,
            content: pending.content,
            reasoning: pending.reasoning,
            tool_calls: pending
                .tool_calls
                .into_iter()
                .map(PendingToolCallPayload::from)
                .collect(),
        }
    }
}

impl From<PendingToolCallTrace> for PendingToolCallPayload {
    fn from(tool_call: PendingToolCallTrace) -> Self {
        Self {
            index: tool_call.index,
            call_id: tool_call.call_id,
            name: tool_call.name,
            arguments_text: tool_call.arguments_text,
            status: PendingToolStatusPayload::from(tool_call.status),
            arguments: tool_call.arguments,
            result: tool_call.result,
            is_error: tool_call.is_error,
        }
    }
}

impl From<PendingToolStatus> for PendingToolStatusPayload {
    fn from(status: PendingToolStatus) -> Self {
        match status {
            PendingToolStatus::Pending => Self::Pending,
            PendingToolStatus::Started => Self::Started,
            PendingToolStatus::Completed => Self::Completed,
        }
    }
}

/// Current snapshot of a chat thread.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadSnapshotPayload {
    pub session_id: String,
    pub thread_id: String,
    pub messages: Vec<ChatMessagePayload>,
    pub turn_count: u32,
    pub token_count: u32,
    pub plan_item_count: usize,
    pub pending_assistant: Option<PendingAssistantPayload>,
}

#[tauri::command]
pub async fn create_chat_session(
    wing: State<'_, Arc<ArgusWing>>,
    subscriptions: State<'_, ThreadSubscriptions>,
    app: tauri::AppHandle,
    template_id: String,
    provider_preference_id: Option<String>,
    model: Option<String>,
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
    let session_id = wing.create_session("").await.map_err(|e| e.to_string())?;

    // Create thread with template, provider, and optional model override
    let thread_id = wing
        .create_thread(
            session_id,
            AgentId::new(template_id_i64),
            provider_id,
            model.as_deref(),
        )
        .await
        .map_err(|e| e.to_string())?;

    let (effective_template_id, effective_provider_id, effective_model) = wing
        .activate_thread(session_id, thread_id)
        .await
        .map_err(|e| e.to_string())?;

    let session_key = format!(
        "{}::{}",
        template_id,
        provider_preference_id.as_deref().unwrap_or("__default__")
    );

    // Start event forwarder
    subscriptions
        .start_forwarder(
            session_id.to_string(),
            session_id,
            thread_id,
            app,
            wing.inner().clone(),
        )
        .await
        .map_err(|e| e.to_string())?;

    Ok(ChatSessionPayload {
        session_key,
        session_id: session_id.to_string(),
        template_id: effective_template_id.into_inner(),
        thread_id: thread_id.to_string(),
        effective_provider_id: effective_provider_id.map(|id| id.inner()),
        effective_model,
    })
}

#[tauri::command]
pub async fn activate_existing_thread(
    wing: State<'_, Arc<ArgusWing>>,
    subscriptions: State<'_, ThreadSubscriptions>,
    app: tauri::AppHandle,
    session_id: String,
    thread_id: String,
) -> Result<ChatSessionPayload, String> {
    let session_id = SessionId::parse(&session_id).map_err(|e| e.to_string())?;
    let thread_id = ThreadId::parse(&thread_id).map_err(|e| e.to_string())?;

    let (template_id, provider_id, effective_model) = wing
        .activate_thread(session_id, thread_id)
        .await
        .map_err(|e| e.to_string())?;

    subscriptions
        .start_forwarder(
            session_id.to_string(),
            session_id,
            thread_id,
            app,
            wing.inner().clone(),
        )
        .await
        .map_err(|e| e.to_string())?;

    Ok(ChatSessionPayload {
        session_key: session_id.to_string(),
        session_id: session_id.to_string(),
        template_id: template_id.into_inner(),
        thread_id: thread_id.to_string(),
        effective_provider_id: provider_id.map(|id| id.inner()),
        effective_model,
    })
}

#[tauri::command]
pub async fn update_thread_model(
    wing: State<'_, Arc<ArgusWing>>,
    session_id: String,
    thread_id: String,
    provider_preference_id: String,
    model: String,
) -> Result<ChatSessionPayload, String> {
    let session_id = SessionId::parse(&session_id).map_err(|e| e.to_string())?;
    let thread_id = ThreadId::parse(&thread_id).map_err(|e| e.to_string())?;
    let provider_id = provider_preference_id
        .parse::<i64>()
        .map(ProviderId::new)
        .map_err(|e| format!("Invalid provider id: {}", e))?;

    wing.update_thread_model(session_id, thread_id, provider_id, &model)
        .await
        .map_err(|e| e.to_string())?;

    let (template_id, effective_provider_id, effective_model) = wing
        .activate_thread(session_id, thread_id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(ChatSessionPayload {
        session_key: session_id.to_string(),
        session_id: session_id.to_string(),
        template_id: template_id.into_inner(),
        thread_id: thread_id.to_string(),
        effective_provider_id: effective_provider_id.map(|id| id.inner()),
        effective_model,
    })
}

#[tauri::command]
pub async fn send_message(
    wing: State<'_, Arc<ArgusWing>>,
    session_id: String,
    thread_id: String,
    content: String,
) -> Result<(), String> {
    let session_id = SessionId::parse(&session_id).map_err(|e| e.to_string())?;
    let thread_id = ThreadId::parse(&thread_id).map_err(|e| e.to_string())?;
    wing.send_message(session_id, thread_id, content)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn cancel_turn(
    wing: State<'_, Arc<ArgusWing>>,
    session_id: String,
    thread_id: String,
) -> Result<(), String> {
    let session_id = SessionId::parse(&session_id).map_err(|e| e.to_string())?;
    let thread_id = ThreadId::parse(&thread_id).map_err(|e| e.to_string())?;
    wing.cancel_turn(session_id, thread_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub fn stop_job(wing: State<'_, Arc<ArgusWing>>, job_id: String) -> Result<(), String> {
    wing.stop_job(job_id).map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_thread_snapshot(
    wing: State<'_, Arc<ArgusWing>>,
    session_id: String,
    thread_id: String,
) -> Result<ThreadSnapshotPayload, String> {
    let session_id = SessionId::parse(&session_id).map_err(|e| e.to_string())?;
    let thread_id = ThreadId::parse(&thread_id).map_err(|e| e.to_string())?;

    let snapshot = wing
        .get_thread_snapshot(session_id, thread_id)
        .await
        .map_err(|e| e.to_string())?;

    Ok(ThreadSnapshotPayload {
        session_id: session_id.to_string(),
        thread_id: thread_id.to_string(),
        messages: snapshot
            .messages
            .iter()
            .map(ChatMessagePayload::from)
            .collect(),
        turn_count: snapshot.turn_count,
        token_count: snapshot.token_count,
        plan_item_count: snapshot.plan_item_count as usize,
        pending_assistant: snapshot.pending_assistant.map(PendingAssistantPayload::from),
    })
}

/// Thread summary for listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadSummaryPayload {
    pub thread_id: String,
    pub title: Option<String>,
    pub turn_count: i64,
    pub token_count: i64,
    pub updated_at: String,
}

#[tauri::command]
pub async fn list_sessions(
    wing: State<'_, Arc<ArgusWing>>,
) -> Result<Vec<SessionSummaryPayload>, String> {
    wing.list_sessions()
        .await
        .map_err(|e| e.to_string())
        .map(|sessions| {
            sessions
                .into_iter()
                .map(|s| SessionSummaryPayload {
                    id: s.id.to_string(),
                    name: s.name,
                    thread_count: s.thread_count,
                    updated_at: s.updated_at.to_rfc3339(),
                })
                .collect()
        })
}

#[tauri::command]
pub async fn delete_session(
    wing: State<'_, Arc<ArgusWing>>,
    session_id: String,
) -> Result<(), String> {
    let session_id = SessionId::parse(&session_id).map_err(|e| e.to_string())?;
    wing.delete_session(session_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn rename_session(
    wing: State<'_, Arc<ArgusWing>>,
    session_id: String,
    name: String,
) -> Result<(), String> {
    let session_id = SessionId::parse(&session_id).map_err(|e| e.to_string())?;
    wing.rename_session(session_id, name)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_threads(
    wing: State<'_, Arc<ArgusWing>>,
    session_id: String,
) -> Result<Vec<ThreadSummaryPayload>, String> {
    let session_id = SessionId::parse(&session_id).map_err(|e| e.to_string())?;
    wing.list_threads(session_id)
        .await
        .map_err(|e| e.to_string())
        .map(|threads| {
            threads
                .into_iter()
                .map(|t| ThreadSummaryPayload {
                    thread_id: t.id.to_string(),
                    title: t.title,
                    turn_count: t.turn_count,
                    token_count: t.token_count,
                    updated_at: t.updated_at.to_rfc3339(),
                })
                .collect()
        })
}

#[tauri::command]
pub async fn rename_thread(
    wing: State<'_, Arc<ArgusWing>>,
    session_id: String,
    thread_id: String,
    title: String,
) -> Result<(), String> {
    let session_id = SessionId::parse(&session_id).map_err(|e| e.to_string())?;
    let thread_id = ThreadId::parse(&thread_id).map_err(|e| e.to_string())?;
    wing.rename_thread(session_id, thread_id, title)
        .await
        .map_err(|e| e.to_string())
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

#[tauri::command]
pub async fn get_provider_context_window(
    wing: State<'_, Arc<ArgusWing>>,
    provider_id: i64,
) -> Result<u32, String> {
    let id = LlmProviderId::new(provider_id);
    match wing.get_provider(id).await {
        Ok(provider) => Ok(provider.context_window()),
        Err(_) => Ok(128_000), // provider not found or build failed, use default
    }
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
            model_config: HashMap::new(),
            default_model: "gpt-4.1".to_string(),
            is_default: false,
            extra_headers: HashMap::new(),
            secret_status: ProviderSecretStatus::RequiresReentry,
            meta_data: HashMap::new(),
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
        use super::{ChatSessionPayload, SessionId};

        let payload = ChatSessionPayload {
            session_key: "arguswing::__default__".to_string(),
            session_id: SessionId::new().to_string(),
            template_id: 1,
            thread_id: ThreadId::new().to_string(),
            effective_provider_id: Some(1),
            effective_model: None,
        };

        let value = serde_json::to_value(payload).expect("payload should serialize");
        assert_eq!(value["effective_provider_id"], json!(1));
        assert_eq!(value["session_key"], json!("arguswing::__default__"));
        assert!(value["session_id"].is_string());
    }

    #[test]
    fn chat_session_payload_serializes_none_effective_provider_id() {
        use super::{ChatSessionPayload, SessionId};

        let payload = ChatSessionPayload {
            session_key: "arguswing::__default__".to_string(),
            session_id: SessionId::new().to_string(),
            template_id: 1,
            thread_id: ThreadId::new().to_string(),
            effective_provider_id: None,
            effective_model: None,
        };

        let value = serde_json::to_value(payload).expect("payload should serialize");
        assert_eq!(value["effective_provider_id"], json!(null));
    }
}
