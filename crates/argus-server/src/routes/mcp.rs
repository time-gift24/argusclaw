use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use argus_protocol::McpServerRecord;

use crate::app_state::AppState;
use crate::error::ApiError;
use crate::response::MutationResponse;

pub async fn list_mcp_servers(
    State(state): State<AppState>,
) -> Result<Json<Vec<McpServerRecord>>, ApiError> {
    Ok(Json(state.wing().list_mcp_servers().await?))
}

pub async fn create_mcp_server(
    State(state): State<AppState>,
    Json(mut record): Json<McpServerRecord>,
) -> Result<(StatusCode, Json<MutationResponse<McpServerRecord>>), ApiError> {
    record.id = None;
    let id = state.wing().upsert_mcp_server(record).await?;
    let saved =
        state.wing().get_mcp_server(id).await?.ok_or_else(|| {
            ApiError::internal(format!("MCP server not found after upsert: {id}"))
        })?;
    Ok((StatusCode::CREATED, Json(MutationResponse::new(saved))))
}

pub async fn update_mcp_server(
    State(state): State<AppState>,
    Path(server_id): Path<i64>,
    Json(mut record): Json<McpServerRecord>,
) -> Result<Json<MutationResponse<McpServerRecord>>, ApiError> {
    record.id = Some(server_id);
    let id = state.wing().upsert_mcp_server(record).await?;
    let saved =
        state.wing().get_mcp_server(id).await?.ok_or_else(|| {
            ApiError::internal(format!("MCP server not found after upsert: {id}"))
        })?;
    Ok(Json(MutationResponse::new(saved)))
}
