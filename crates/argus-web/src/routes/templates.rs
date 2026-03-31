use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::routing::{delete, get};
use axum::Json;
use argus_protocol::{AgentId, AgentRecord};

use crate::error::ApiError;
use crate::state::AppState;

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/templates", get(list_templates).post(upsert_template))
        .route("/templates/default", get(get_default_template))
        .route(
            "/templates/{id}",
            get(get_template).delete(delete_template),
        )
        .route(
            "/templates/{id}/subagents",
            get(list_subagents).post(add_subagent),
        )
        .route(
            "/templates/{id}/subagents/{child_id}",
            delete(remove_subagent),
        )
}

async fn list_templates(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let templates = state.wing.list_templates().await.map_err(ApiError::from)?;
    Ok(Json(templates))
}

async fn get_default_template(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let template = state
        .wing
        .get_default_template()
        .await
        .map_err(ApiError::from)?;
    Ok(Json(template))
}

async fn get_template(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, ApiError> {
    let template = state
        .wing
        .get_template(AgentId::new(id))
        .await
        .map_err(ApiError::from)?;
    match template {
        Some(t) => Ok(Json(t)),
        None => Err(ApiError::NotFound(format!(
            "Template {id} not found"
        ))),
    }
}

async fn upsert_template(
    State(state): State<AppState>,
    Json(body): Json<AgentRecord>,
) -> Result<impl IntoResponse, ApiError> {
    let id = state
        .wing
        .upsert_template(body)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({ "id": id.to_string() })))
}

async fn delete_template(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, ApiError> {
    state
        .wing
        .delete_template(AgentId::new(id))
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({ "deleted": true })))
}

async fn list_subagents(
    State(state): State<AppState>,
    Path(id): Path<i64>,
) -> Result<impl IntoResponse, ApiError> {
    let subagents = state
        .wing
        .list_subagents(AgentId::new(id))
        .await
        .map_err(ApiError::from)?;
    Ok(Json(subagents))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct AddSubagentBody {
    pub child_id: i64,
}

async fn add_subagent(
    State(state): State<AppState>,
    Path(id): Path<i64>,
    Json(body): Json<AddSubagentBody>,
) -> Result<impl IntoResponse, ApiError> {
    state
        .wing
        .add_subagent(AgentId::new(id), AgentId::new(body.child_id))
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({ "success": true })))
}

async fn remove_subagent(
    State(state): State<AppState>,
    Path((id, child_id)): Path<(i64, i64)>,
) -> Result<impl IntoResponse, ApiError> {
    state
        .wing
        .remove_subagent(AgentId::new(id), AgentId::new(child_id))
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({ "success": true })))
}
