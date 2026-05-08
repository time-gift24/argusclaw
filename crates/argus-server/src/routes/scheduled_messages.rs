use argus_protocol::{SessionId, ThreadId};
use argus_session::scheduled_messages::{CreateScheduledMessageRequest, ScheduledMessageSummary};
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::app_state::AppState;
use crate::error::ApiError;
use crate::response::{DeleteResponse, MutationResponse};
use crate::user_context::RequestUser;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateScheduledMessageBody {
    pub session_id: String,
    pub thread_id: String,
    pub name: String,
    pub prompt: String,
    pub cron_expr: Option<String>,
    pub scheduled_at: Option<String>,
    pub timezone: Option<String>,
}

pub async fn list_scheduled_messages(
    request_user: RequestUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<ScheduledMessageSummary>>, ApiError> {
    Ok(Json(
        state.core().list_scheduled_messages(&request_user).await?,
    ))
}

pub async fn create_scheduled_message(
    request_user: RequestUser,
    State(state): State<AppState>,
    Json(body): Json<CreateScheduledMessageBody>,
) -> Result<(StatusCode, Json<MutationResponse<ScheduledMessageSummary>>), ApiError> {
    let summary = state
        .core()
        .create_scheduled_message(
            &request_user,
            CreateScheduledMessageRequest {
                session_id: parse_session_id(&body.session_id)?,
                thread_id: parse_thread_id(&body.thread_id)?,
                name: body.name,
                prompt: body.prompt,
                cron_expr: body.cron_expr,
                scheduled_at: body.scheduled_at,
                timezone: body.timezone,
            },
        )
        .await?;

    Ok((StatusCode::CREATED, Json(MutationResponse::new(summary))))
}

pub async fn pause_scheduled_message(
    request_user: RequestUser,
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<MutationResponse<ScheduledMessageSummary>>, ApiError> {
    let summary = state
        .core()
        .pause_scheduled_message(&request_user, &job_id)
        .await?;
    Ok(Json(MutationResponse::new(summary)))
}

pub async fn trigger_scheduled_message(
    request_user: RequestUser,
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<MutationResponse<ScheduledMessageSummary>>, ApiError> {
    let summary = state
        .core()
        .trigger_scheduled_message_now(&request_user, &job_id)
        .await?;
    Ok(Json(MutationResponse::new(summary)))
}

pub async fn delete_scheduled_message(
    request_user: RequestUser,
    State(state): State<AppState>,
    Path(job_id): Path<String>,
) -> Result<Json<MutationResponse<DeleteResponse>>, ApiError> {
    let deleted = state
        .core()
        .delete_scheduled_message(&request_user, &job_id)
        .await?;
    Ok(Json(MutationResponse::new(DeleteResponse { deleted })))
}

fn parse_session_id(value: &str) -> Result<SessionId, ApiError> {
    SessionId::parse(value).map_err(|_| ApiError::bad_request("invalid session_id"))
}

fn parse_thread_id(value: &str) -> Result<ThreadId, ApiError> {
    ThreadId::parse(value).map_err(|_| ApiError::bad_request("invalid thread_id"))
}
