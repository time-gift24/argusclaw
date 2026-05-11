use argus_protocol::{AgentId, ProviderId};
use argus_session::scheduled_messages::{
    CreateScheduledMessageRequest, ScheduledMessageSummary, UpdateScheduledMessageRequest,
};
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
    pub template_id: i64,
    pub provider_id: Option<i64>,
    pub model: Option<String>,
    pub name: String,
    pub prompt: String,
    pub cron_expr: Option<String>,
    pub scheduled_at: Option<String>,
    pub timezone: Option<String>,
}

pub type UpdateScheduledMessageBody = CreateScheduledMessageBody;

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
                owner_user_id: state.core().chat_user_id(&request_user).await?,
                template_id: AgentId::new(body.template_id),
                provider_id: body.provider_id.map(ProviderId::new),
                model: body.model,
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

pub async fn update_scheduled_message(
    request_user: RequestUser,
    State(state): State<AppState>,
    Path(job_id): Path<String>,
    Json(body): Json<UpdateScheduledMessageBody>,
) -> Result<Json<MutationResponse<ScheduledMessageSummary>>, ApiError> {
    let summary = state
        .core()
        .update_scheduled_message(
            &request_user,
            &job_id,
            UpdateScheduledMessageRequest {
                template_id: AgentId::new(body.template_id),
                provider_id: body.provider_id.map(ProviderId::new),
                model: body.model,
                name: body.name,
                prompt: body.prompt,
                cron_expr: body.cron_expr,
                scheduled_at: body.scheduled_at,
                timezone: body.timezone,
            },
        )
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
