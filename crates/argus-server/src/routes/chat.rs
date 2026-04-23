use argus_protocol::llm::ChatMessage;
use argus_protocol::{AgentId, ProviderId, SessionId, ThreadId};
use argus_session::{SessionSummary, ThreadSummary};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::app_state::AppState;
use crate::error::ApiError;
use crate::response::{DeleteResponse, MutationResponse};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateSessionRequest {
    pub name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateThreadRequest {
    pub template_id: i64,
    pub provider_id: Option<i64>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendMessageRequest {
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatActionResponse {
    pub accepted: bool,
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
    let session = state.core().create_chat_session(request.name).await?;
    Ok((StatusCode::CREATED, Json(MutationResponse::new(session))))
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
            request.message,
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

fn parse_session_id(value: &str) -> Result<SessionId, ApiError> {
    SessionId::parse(value)
        .map_err(|error| ApiError::internal(format!("Invalid session_id '{value}': {error}")))
}

fn parse_thread_id(value: &str) -> Result<ThreadId, ApiError> {
    ThreadId::parse(value)
        .map_err(|error| ApiError::internal(format!("Invalid thread_id '{value}': {error}")))
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
