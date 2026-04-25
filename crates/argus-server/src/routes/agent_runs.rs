use std::collections::BTreeMap;

use argus_protocol::AgentId;
use argus_repository::types::AgentRunId;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::app_state::AppState;
use crate::error::ApiError;
use crate::server_core::{AgentRunDetail, AgentRunSummary, McpHeaderOverrideError};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateAgentRunRequest {
    pub agent_id: i64,
    pub prompt: String,
    #[serde(default)]
    pub mcp_headers: BTreeMap<String, BTreeMap<String, String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateAgentRunResponse {
    pub data: AgentRunSummary,
}

pub async fn create_agent_run(
    State(state): State<AppState>,
    Json(request): Json<CreateAgentRunRequest>,
) -> Result<(StatusCode, Json<CreateAgentRunResponse>), ApiError> {
    let prompt = required_non_empty("prompt", request.prompt)?;
    let mcp_headers = state
        .core()
        .resolve_mcp_header_overrides(request.mcp_headers)
        .await
        .map_err(mcp_header_override_error)?;
    let run = state
        .core()
        .create_agent_run(AgentId::new(request.agent_id), prompt, mcp_headers)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(CreateAgentRunResponse { data: run }),
    ))
}

pub async fn get_agent_run(
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<AgentRunDetail>, ApiError> {
    Ok(Json(
        state.core().get_agent_run(parse_run_id(&run_id)?).await?,
    ))
}

fn mcp_header_override_error(error: McpHeaderOverrideError) -> ApiError {
    match error {
        McpHeaderOverrideError::BadRequest(message) => ApiError::bad_request(message),
        McpHeaderOverrideError::Internal(error) => ApiError::from(error),
    }
}

fn parse_run_id(value: &str) -> Result<AgentRunId, ApiError> {
    AgentRunId::parse(value)
        .map_err(|error| ApiError::bad_request(format!("Invalid run_id '{value}': {error}")))
}

fn required_non_empty(field: &str, value: String) -> Result<String, ApiError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        Err(ApiError::bad_request(format!("{field} must not be empty")))
    } else {
        Ok(trimmed.to_string())
    }
}
