use std::convert::Infallible;

use argus_protocol::llm::ChatMessage;
use argus_protocol::{
    AgentId, JobRuntimeSnapshot, JobRuntimeSummary, LlmStreamEvent, MailboxMessage, ProviderId,
    SessionId, ThreadEvent, ThreadId, ThreadNoticeLevel, ThreadPoolEventReason, ThreadPoolSnapshot,
};
use argus_session::{SessionSummary, ThreadSummary};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::response::sse::{Event, KeepAlive, Sse};
use futures_util::Stream;
use futures_util::stream;
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::app_state::AppState;
use crate::error::ApiError;
use crate::response::{DeleteResponse, MutationResponse};
use crate::server_core::{ChatSessionPayload, ChatThreadBinding, ChatThreadSnapshot};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateSessionRequest {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameSessionRequest {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateThreadRequest {
    pub template_id: i64,
    pub provider_id: Option<i64>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RenameThreadRequest {
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateThreadModelRequest {
    pub provider_id: i64,
    pub model: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendMessageRequest {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatActionResponse {
    pub accepted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateSessionWithThreadRequest {
    pub template_id: i64,
    pub provider_id: Option<i64>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatThreadEventEnvelope {
    pub session_id: String,
    pub thread_id: String,
    pub turn_number: Option<u32>,
    pub payload: ChatThreadEventPayload,
}

impl ChatThreadEventEnvelope {
    fn from_thread_event(session_id: String, event: ThreadEvent) -> Option<Self> {
        match event {
            ThreadEvent::Processing {
                thread_id,
                turn_number,
                event,
            } => ChatThreadEventPayload::from_llm_event(event).map(|payload| Self {
                session_id,
                thread_id,
                turn_number: Some(turn_number),
                payload,
            }),
            ThreadEvent::ToolStarted {
                thread_id,
                turn_number,
                tool_call_id,
                tool_name,
                arguments,
            } => Some(Self {
                session_id,
                thread_id,
                turn_number: Some(turn_number),
                payload: ChatThreadEventPayload::ToolStarted {
                    tool_call_id,
                    tool_name,
                    arguments,
                },
            }),
            ThreadEvent::ToolCompleted {
                thread_id,
                turn_number,
                tool_call_id,
                tool_name,
                result,
            } => {
                let (result, is_error) = match result {
                    Ok(result) => (result, false),
                    Err(error) => (serde_json::Value::String(error), true),
                };

                Some(Self {
                    session_id,
                    thread_id,
                    turn_number: Some(turn_number),
                    payload: ChatThreadEventPayload::ToolCompleted {
                        tool_call_id,
                        tool_name,
                        result,
                        is_error,
                    },
                })
            }
            ThreadEvent::TurnCompleted {
                thread_id,
                turn_number,
                token_usage,
            } => Some(Self {
                session_id,
                thread_id,
                turn_number: Some(turn_number),
                payload: ChatThreadEventPayload::TurnCompleted {
                    input_tokens: token_usage.input_tokens,
                    output_tokens: token_usage.output_tokens,
                    total_tokens: token_usage.total_tokens,
                },
            }),
            ThreadEvent::TurnFailed {
                thread_id,
                turn_number,
                error,
            } => Some(Self {
                session_id,
                thread_id,
                turn_number: Some(turn_number),
                payload: ChatThreadEventPayload::TurnFailed { error },
            }),
            ThreadEvent::TurnSettled {
                thread_id,
                turn_number,
            } => Some(Self {
                session_id,
                thread_id,
                turn_number: Some(turn_number),
                payload: ChatThreadEventPayload::TurnSettled,
            }),
            ThreadEvent::Idle { thread_id } => Some(Self {
                session_id,
                thread_id,
                turn_number: None,
                payload: ChatThreadEventPayload::Idle,
            }),
            ThreadEvent::Notice {
                thread_id,
                level,
                message,
            } => Some(Self {
                session_id,
                thread_id,
                turn_number: None,
                payload: ChatThreadEventPayload::Notice { level, message },
            }),
            ThreadEvent::Compacted {
                thread_id,
                new_token_count,
            } => Some(Self {
                session_id,
                thread_id,
                turn_number: None,
                payload: ChatThreadEventPayload::Compacted { new_token_count },
            }),
            ThreadEvent::CompactionStarted { thread_id } => Some(Self {
                session_id,
                thread_id,
                turn_number: None,
                payload: ChatThreadEventPayload::CompactionStarted,
            }),
            ThreadEvent::CompactionFinished { thread_id } => Some(Self {
                session_id,
                thread_id,
                turn_number: None,
                payload: ChatThreadEventPayload::CompactionFinished,
            }),
            ThreadEvent::CompactionFailed { thread_id, error } => Some(Self {
                session_id,
                thread_id,
                turn_number: None,
                payload: ChatThreadEventPayload::CompactionFailed { error },
            }),
            ThreadEvent::JobDispatched {
                thread_id,
                job_id,
                agent_id,
                prompt,
                context,
            } => Some(Self {
                session_id,
                thread_id: thread_id.to_string(),
                turn_number: None,
                payload: ChatThreadEventPayload::JobDispatched {
                    job_id,
                    agent_id: agent_id.inner(),
                    prompt,
                    context,
                },
            }),
            ThreadEvent::JobResult {
                thread_id,
                job_id,
                success,
                cancelled,
                message,
                token_usage,
                agent_id,
                agent_display_name,
                agent_description,
            } => Some(Self {
                session_id,
                thread_id: thread_id.to_string(),
                turn_number: None,
                payload: ChatThreadEventPayload::JobResult {
                    job_id,
                    success,
                    cancelled,
                    message,
                    input_tokens: token_usage.as_ref().map(|usage| usage.input_tokens),
                    output_tokens: token_usage.as_ref().map(|usage| usage.output_tokens),
                    agent_id: agent_id.inner(),
                    agent_display_name,
                    agent_description,
                },
            }),
            ThreadEvent::MailboxMessageQueued { thread_id, message } => Some(Self {
                session_id,
                thread_id: thread_id.to_string(),
                turn_number: None,
                payload: ChatThreadEventPayload::MailboxMessageQueued { message },
            }),
            ThreadEvent::ThreadBoundToJob { job_id, thread_id } => Some(Self {
                session_id,
                thread_id: thread_id.to_string(),
                turn_number: None,
                payload: ChatThreadEventPayload::ThreadBoundToJob { job_id },
            }),
            ThreadEvent::ThreadPoolQueued {
                thread_id,
                session_id: runtime_session_id,
            } => Some(Self {
                session_id,
                thread_id: thread_id.to_string(),
                turn_number: None,
                payload: ChatThreadEventPayload::ThreadPoolQueued {
                    session_id: runtime_session_id,
                },
            }),
            ThreadEvent::ThreadPoolStarted {
                thread_id,
                session_id: runtime_session_id,
            } => Some(Self {
                session_id,
                thread_id: thread_id.to_string(),
                turn_number: None,
                payload: ChatThreadEventPayload::ThreadPoolStarted {
                    session_id: runtime_session_id,
                },
            }),
            ThreadEvent::ThreadPoolCooling {
                thread_id,
                session_id: runtime_session_id,
            } => Some(Self {
                session_id,
                thread_id: thread_id.to_string(),
                turn_number: None,
                payload: ChatThreadEventPayload::ThreadPoolCooling {
                    session_id: runtime_session_id,
                },
            }),
            ThreadEvent::ThreadPoolEvicted {
                thread_id,
                session_id: runtime_session_id,
                reason,
            } => Some(Self {
                session_id,
                thread_id: thread_id.to_string(),
                turn_number: None,
                payload: ChatThreadEventPayload::ThreadPoolEvicted {
                    session_id: runtime_session_id,
                    reason,
                },
            }),
            ThreadEvent::ThreadPoolMetricsUpdated { snapshot } => Some(Self {
                session_id,
                thread_id: String::new(),
                turn_number: None,
                payload: ChatThreadEventPayload::ThreadPoolMetricsUpdated { snapshot },
            }),
            ThreadEvent::JobRuntimeQueued { thread_id, job_id } => Some(Self {
                session_id,
                thread_id: thread_id.to_string(),
                turn_number: None,
                payload: ChatThreadEventPayload::JobRuntimeQueued { job_id },
            }),
            ThreadEvent::JobRuntimeStarted { thread_id, job_id } => Some(Self {
                session_id,
                thread_id: thread_id.to_string(),
                turn_number: None,
                payload: ChatThreadEventPayload::JobRuntimeStarted { job_id },
            }),
            ThreadEvent::JobRuntimeCooling { thread_id, job_id } => Some(Self {
                session_id,
                thread_id: thread_id.to_string(),
                turn_number: None,
                payload: ChatThreadEventPayload::JobRuntimeCooling { job_id },
            }),
            ThreadEvent::JobRuntimeEvicted {
                thread_id,
                job_id,
                reason,
            } => Some(Self {
                session_id,
                thread_id: thread_id.to_string(),
                turn_number: None,
                payload: ChatThreadEventPayload::JobRuntimeEvicted { job_id, reason },
            }),
            ThreadEvent::JobRuntimeUpdated { runtime } => Some(Self {
                session_id,
                thread_id: runtime.thread_id.to_string(),
                turn_number: None,
                payload: ChatThreadEventPayload::JobRuntimeUpdated { runtime },
            }),
            ThreadEvent::JobRuntimeMetricsUpdated { snapshot } => Some(Self {
                session_id,
                thread_id: String::new(),
                turn_number: None,
                payload: ChatThreadEventPayload::JobRuntimeMetricsUpdated { snapshot },
            }),
            ThreadEvent::UserInterrupt { .. } | ThreadEvent::UserMessage { .. } => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ChatThreadEventPayload {
    ReasoningDelta {
        delta: String,
    },
    ContentDelta {
        delta: String,
    },
    ToolCallDelta {
        index: usize,
        id: Option<String>,
        name: Option<String>,
        arguments_delta: Option<String>,
    },
    LlmUsage {
        input_tokens: u32,
        output_tokens: u32,
        total_tokens: u32,
    },
    RetryAttempt {
        attempt: u32,
        max_retries: u32,
        error: String,
    },
    ToolStarted {
        tool_call_id: String,
        tool_name: String,
        arguments: serde_json::Value,
    },
    ToolCompleted {
        tool_call_id: String,
        tool_name: String,
        result: serde_json::Value,
        is_error: bool,
    },
    TurnCompleted {
        input_tokens: u32,
        output_tokens: u32,
        total_tokens: u32,
    },
    TurnFailed {
        error: String,
    },
    TurnSettled,
    Idle,
    Notice {
        level: ThreadNoticeLevel,
        message: String,
    },
    Compacted {
        new_token_count: u32,
    },
    CompactionStarted,
    CompactionFinished,
    CompactionFailed {
        error: String,
    },
    ThreadBoundToJob {
        job_id: String,
    },
    ThreadPoolQueued {
        session_id: Option<SessionId>,
    },
    ThreadPoolStarted {
        session_id: Option<SessionId>,
    },
    ThreadPoolCooling {
        session_id: Option<SessionId>,
    },
    ThreadPoolEvicted {
        session_id: Option<SessionId>,
        reason: ThreadPoolEventReason,
    },
    ThreadPoolMetricsUpdated {
        snapshot: ThreadPoolSnapshot,
    },
    JobRuntimeQueued {
        job_id: String,
    },
    JobRuntimeStarted {
        job_id: String,
    },
    JobRuntimeCooling {
        job_id: String,
    },
    JobRuntimeEvicted {
        job_id: String,
        reason: ThreadPoolEventReason,
    },
    JobRuntimeUpdated {
        runtime: JobRuntimeSummary,
    },
    JobRuntimeMetricsUpdated {
        snapshot: JobRuntimeSnapshot,
    },
    JobDispatched {
        job_id: String,
        agent_id: i64,
        prompt: String,
        context: Option<serde_json::Value>,
    },
    JobResult {
        job_id: String,
        success: bool,
        cancelled: bool,
        message: String,
        input_tokens: Option<u32>,
        output_tokens: Option<u32>,
        agent_id: i64,
        agent_display_name: String,
        agent_description: String,
    },
    MailboxMessageQueued {
        message: MailboxMessage,
    },
}

impl ChatThreadEventPayload {
    fn from_llm_event(event: LlmStreamEvent) -> Option<Self> {
        match event {
            LlmStreamEvent::ReasoningDelta { delta } => Some(Self::ReasoningDelta { delta }),
            LlmStreamEvent::ContentDelta { delta } => Some(Self::ContentDelta { delta }),
            LlmStreamEvent::ToolCallDelta(delta) => Some(Self::ToolCallDelta {
                index: delta.index,
                id: delta.id,
                name: delta.name,
                arguments_delta: delta.arguments_delta,
            }),
            LlmStreamEvent::Usage {
                input_tokens,
                output_tokens,
            } => Some(Self::LlmUsage {
                input_tokens,
                output_tokens,
                total_tokens: input_tokens + output_tokens,
            }),
            LlmStreamEvent::RetryAttempt {
                attempt,
                max_retries,
                error,
            } => Some(Self::RetryAttempt {
                attempt,
                max_retries,
                error,
            }),
            LlmStreamEvent::Finished { .. } => None,
        }
    }
}

pub async fn list_sessions(
    State(state): State<AppState>,
) -> Result<Json<Vec<SessionSummary>>, ApiError> {
    Ok(Json(state.core().list_chat_sessions().await?))
}

pub async fn create_session(
    State(state): State<AppState>,
    Json(request): Json<CreateSessionRequest>,
) -> Result<(StatusCode, Json<MutationResponse<SessionSummary>>), ApiError> {
    let session = state
        .core()
        .create_chat_session(required_non_empty("name", request.name)?)
        .await?;
    Ok((StatusCode::CREATED, Json(MutationResponse::new(session))))
}

pub async fn create_session_with_thread(
    State(state): State<AppState>,
    Json(request): Json<CreateSessionWithThreadRequest>,
) -> Result<(StatusCode, Json<MutationResponse<ChatSessionPayload>>), ApiError> {
    let payload = state
        .core()
        .create_chat_session_with_thread(
            AgentId::new(request.template_id),
            request.provider_id.map(ProviderId::new),
            normalize_optional_string(request.model),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(MutationResponse::new(payload))))
}

pub async fn rename_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(request): Json<RenameSessionRequest>,
) -> Result<Json<MutationResponse<SessionSummary>>, ApiError> {
    let session = state
        .core()
        .rename_chat_session(
            parse_session_id(&session_id)?,
            required_non_empty("name", request.name)?,
        )
        .await?;
    Ok(Json(MutationResponse::new(session)))
}

pub async fn delete_session(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<MutationResponse<DeleteResponse>>, ApiError> {
    state
        .core()
        .delete_chat_session(parse_session_id(&session_id)?)
        .await?;
    Ok(Json(MutationResponse::new(DeleteResponse {
        deleted: true,
    })))
}

pub async fn list_threads(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
) -> Result<Json<Vec<ThreadSummary>>, ApiError> {
    Ok(Json(
        state
            .core()
            .list_chat_threads(parse_session_id(&session_id)?)
            .await?,
    ))
}

pub async fn create_thread(
    State(state): State<AppState>,
    Path(session_id): Path<String>,
    Json(request): Json<CreateThreadRequest>,
) -> Result<(StatusCode, Json<MutationResponse<ThreadSummary>>), ApiError> {
    let thread = state
        .core()
        .create_chat_thread(
            parse_session_id(&session_id)?,
            AgentId::new(request.template_id),
            request.provider_id.map(ProviderId::new),
            normalize_optional_string(request.model),
        )
        .await?;
    Ok((StatusCode::CREATED, Json(MutationResponse::new(thread))))
}

pub async fn delete_thread(
    State(state): State<AppState>,
    Path((session_id, thread_id)): Path<(String, String)>,
) -> Result<Json<MutationResponse<DeleteResponse>>, ApiError> {
    state
        .core()
        .delete_chat_thread(parse_session_id(&session_id)?, parse_thread_id(&thread_id)?)
        .await?;
    Ok(Json(MutationResponse::new(DeleteResponse {
        deleted: true,
    })))
}

pub async fn get_thread_snapshot(
    State(state): State<AppState>,
    Path((session_id, thread_id)): Path<(String, String)>,
) -> Result<Json<ChatThreadSnapshot>, ApiError> {
    Ok(Json(
        state
            .core()
            .get_chat_thread_snapshot(parse_session_id(&session_id)?, parse_thread_id(&thread_id)?)
            .await?,
    ))
}

pub async fn rename_thread(
    State(state): State<AppState>,
    Path((session_id, thread_id)): Path<(String, String)>,
    Json(request): Json<RenameThreadRequest>,
) -> Result<Json<MutationResponse<ThreadSummary>>, ApiError> {
    let thread = state
        .core()
        .rename_chat_thread(
            parse_session_id(&session_id)?,
            parse_thread_id(&thread_id)?,
            request.title,
        )
        .await?;
    Ok(Json(MutationResponse::new(thread)))
}

pub async fn update_thread_model(
    State(state): State<AppState>,
    Path((session_id, thread_id)): Path<(String, String)>,
    Json(request): Json<UpdateThreadModelRequest>,
) -> Result<Json<MutationResponse<ChatThreadBinding>>, ApiError> {
    let binding = state
        .core()
        .update_chat_thread_model(
            parse_session_id(&session_id)?,
            parse_thread_id(&thread_id)?,
            ProviderId::new(request.provider_id),
            required_non_empty("model", request.model)?,
        )
        .await?;
    Ok(Json(MutationResponse::new(binding)))
}

pub async fn activate_thread(
    State(state): State<AppState>,
    Path((session_id, thread_id)): Path<(String, String)>,
) -> Result<Json<MutationResponse<ChatThreadBinding>>, ApiError> {
    let binding = state
        .core()
        .activate_chat_thread(parse_session_id(&session_id)?, parse_thread_id(&thread_id)?)
        .await?;
    Ok(Json(MutationResponse::new(binding)))
}

pub async fn list_messages(
    State(state): State<AppState>,
    Path((session_id, thread_id)): Path<(String, String)>,
) -> Result<Json<Vec<ChatMessage>>, ApiError> {
    Ok(Json(
        state
            .core()
            .get_chat_messages(parse_session_id(&session_id)?, parse_thread_id(&thread_id)?)
            .await?,
    ))
}

pub async fn send_message(
    State(state): State<AppState>,
    Path((session_id, thread_id)): Path<(String, String)>,
    Json(request): Json<SendMessageRequest>,
) -> Result<(StatusCode, Json<MutationResponse<ChatActionResponse>>), ApiError> {
    state
        .core()
        .send_chat_message(
            parse_session_id(&session_id)?,
            parse_thread_id(&thread_id)?,
            required_non_empty("message", request.message)?,
        )
        .await?;
    Ok((
        StatusCode::ACCEPTED,
        Json(MutationResponse::new(ChatActionResponse { accepted: true })),
    ))
}

pub async fn cancel_thread(
    State(state): State<AppState>,
    Path((session_id, thread_id)): Path<(String, String)>,
) -> Result<Json<MutationResponse<ChatActionResponse>>, ApiError> {
    state
        .core()
        .cancel_chat_thread(parse_session_id(&session_id)?, parse_thread_id(&thread_id)?)
        .await?;
    Ok(Json(MutationResponse::new(ChatActionResponse {
        accepted: true,
    })))
}

pub async fn thread_events(
    State(state): State<AppState>,
    Path((session_id, thread_id)): Path<(String, String)>,
) -> Result<Sse<impl Stream<Item = Result<Event, Infallible>>>, ApiError> {
    let session_id = parse_session_id(&session_id)?;
    let thread_id = parse_thread_id(&thread_id)?;
    let receiver = state
        .core()
        .subscribe_chat_thread(session_id, thread_id)
        .await?;
    let session_id = session_id.to_string();
    let stream = stream::unfold(
        (receiver, session_id),
        |(mut receiver, session_id)| async move {
            loop {
                match receiver.recv().await {
                    Ok(thread_event) => {
                        if let Some(envelope) = ChatThreadEventEnvelope::from_thread_event(
                            session_id.clone(),
                            thread_event,
                        ) {
                            let event = match Event::default()
                                .event("chat.thread_event")
                                .json_data(envelope)
                            {
                                Ok(event) => event,
                                Err(error) => {
                                    Event::default().event("chat.error").data(error.to_string())
                                }
                            };
                            return Some((Ok(event), (receiver, session_id)));
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(skipped)) => {
                        let event = Event::default()
                            .event("chat.error")
                            .data(format!("thread event stream lagged by {skipped} messages"));
                        return Some((Ok(event), (receiver, session_id)));
                    }
                    Err(broadcast::error::RecvError::Closed) => return None,
                }
            }
        },
    );

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

fn parse_session_id(value: &str) -> Result<SessionId, ApiError> {
    SessionId::parse(value)
        .map_err(|error| ApiError::bad_request(format!("Invalid session_id '{value}': {error}")))
}

fn parse_thread_id(value: &str) -> Result<ThreadId, ApiError> {
    ThreadId::parse(value)
        .map_err(|error| ApiError::bad_request(format!("Invalid thread_id '{value}': {error}")))
}

fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value.and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn required_non_empty(field: &str, value: String) -> Result<String, ApiError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(ApiError::bad_request(format!("{field} must not be empty")))
    } else {
        Ok(trimmed.to_string())
    }
}
