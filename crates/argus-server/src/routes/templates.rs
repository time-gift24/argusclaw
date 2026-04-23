use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use argus_protocol::{AgentId, AgentRecord};

use crate::app_state::AppState;
use crate::error::ApiError;
use crate::response::{DeleteResponse, MutationResponse};

pub async fn list_templates(
    State(state): State<AppState>,
) -> Result<Json<Vec<AgentRecord>>, ApiError> {
    Ok(Json(state.core().list_templates().await?))
}

pub async fn create_template(
    State(state): State<AppState>,
    Json(mut record): Json<AgentRecord>,
) -> Result<(StatusCode, Json<MutationResponse<AgentRecord>>), ApiError> {
    record.id = AgentId::new(0);
    let id = state.core().upsert_template(record).await?;
    let saved = state
        .core()
        .get_template(id)
        .await?
        .ok_or_else(|| ApiError::internal(format!("Template not found after upsert: {id}")))?;
    Ok((StatusCode::CREATED, Json(MutationResponse::new(saved))))
}

pub async fn update_template(
    State(state): State<AppState>,
    Path(template_id): Path<i64>,
    Json(mut record): Json<AgentRecord>,
) -> Result<Json<MutationResponse<AgentRecord>>, ApiError> {
    record.id = AgentId::new(template_id);
    let id = state.core().upsert_template(record).await?;
    let saved = state
        .core()
        .get_template(id)
        .await?
        .ok_or_else(|| ApiError::internal(format!("Template not found after upsert: {id}")))?;
    Ok(Json(MutationResponse::new(saved)))
}

pub async fn delete_template(
    State(state): State<AppState>,
    Path(template_id): Path<i64>,
) -> Result<Json<MutationResponse<DeleteResponse>>, ApiError> {
    state
        .core()
        .delete_template(AgentId::new(template_id))
        .await?;
    Ok(Json(MutationResponse::new(DeleteResponse {
        deleted: true,
    })))
}
