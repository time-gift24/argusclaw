use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use argus_protocol::{AgentId, AgentRecord};

use crate::app_state::AppState;
use crate::error::ApiError;
use crate::response::MutationResponse;

pub async fn list_templates(
    State(state): State<AppState>,
) -> Result<Json<Vec<AgentRecord>>, ApiError> {
    Ok(Json(state.wing().list_templates().await?))
}

pub async fn create_template(
    State(state): State<AppState>,
    Json(mut record): Json<AgentRecord>,
) -> Result<(StatusCode, Json<MutationResponse<AgentRecord>>), ApiError> {
    record.id = AgentId::new(0);
    let id = state.wing().upsert_template(record).await?;
    let saved = state
        .wing()
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
    let id = state.wing().upsert_template(record).await?;
    let saved = state
        .wing()
        .get_template(id)
        .await?
        .ok_or_else(|| ApiError::internal(format!("Template not found after upsert: {id}")))?;
    Ok(Json(MutationResponse::new(saved)))
}
