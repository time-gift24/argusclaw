use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;

use argus_protocol::{McpDiscoveredToolRecord, McpServerRecord};

use crate::app_state::AppState;
use crate::error::ApiError;
use crate::response::{DeleteResponse, MutationResponse};

pub async fn list_mcp_servers(
    State(state): State<AppState>,
) -> Result<Json<Vec<McpServerRecord>>, ApiError> {
    Ok(Json(state.core().list_mcp_servers().await?))
}

pub async fn create_mcp_server(
    State(state): State<AppState>,
    Json(mut record): Json<McpServerRecord>,
) -> Result<(StatusCode, Json<MutationResponse<McpServerRecord>>), ApiError> {
    record.id = None;
    let id = state.core().upsert_mcp_server(record).await?;
    let saved =
        state.core().get_mcp_server(id).await?.ok_or_else(|| {
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
    let id = state.core().upsert_mcp_server(record).await?;
    let saved =
        state.core().get_mcp_server(id).await?.ok_or_else(|| {
            ApiError::internal(format!("MCP server not found after upsert: {id}"))
        })?;
    Ok(Json(MutationResponse::new(saved)))
}

pub async fn delete_mcp_server(
    State(state): State<AppState>,
    Path(server_id): Path<i64>,
) -> Result<Json<MutationResponse<DeleteResponse>>, ApiError> {
    let deleted = state.core().delete_mcp_server(server_id).await?;
    Ok(Json(MutationResponse::new(DeleteResponse { deleted })))
}

pub async fn test_mcp_server_connection(
    State(state): State<AppState>,
    Path(server_id): Path<i64>,
) -> Result<Json<argus_mcp::McpConnectionTestResult>, ApiError> {
    Ok(Json(
        state.core().test_mcp_server_connection(server_id).await?,
    ))
}

pub async fn test_mcp_server_input(
    State(state): State<AppState>,
    Json(record): Json<McpServerRecord>,
) -> Result<Json<argus_mcp::McpConnectionTestResult>, ApiError> {
    Ok(Json(state.core().test_mcp_server_input(record).await?))
}

pub async fn list_mcp_server_tools(
    State(state): State<AppState>,
    Path(server_id): Path<i64>,
) -> Result<Json<Vec<McpDiscoveredToolRecord>>, ApiError> {
    Ok(Json(state.core().list_mcp_server_tools(server_id).await?))
}
