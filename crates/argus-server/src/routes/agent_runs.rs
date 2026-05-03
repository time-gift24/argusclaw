use argus_protocol::AgentId;
use argus_repository::types::AgentRunId;
use axum::Json;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use serde::{Deserialize, Serialize};

use crate::app_state::AppState;
use crate::error::ApiError;
use crate::server_core::{AgentRunDetail, AgentRunSummary};
use crate::user_context::RequestUser;

use super::require_admin;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateAgentRunRequest {
    pub agent_id: i64,
    pub prompt: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateAgentRunResponse {
    pub data: AgentRunSummary,
}

pub async fn create_agent_run(
    request_user: RequestUser,
    State(state): State<AppState>,
    Json(request): Json<CreateAgentRunRequest>,
) -> Result<(StatusCode, Json<CreateAgentRunResponse>), ApiError> {
    require_admin(&state, &request_user).await?;
    let prompt = required_non_empty("prompt", request.prompt)?;
    let run = state
        .core()
        .create_agent_run(AgentId::new(request.agent_id), prompt)
        .await?;
    Ok((
        StatusCode::CREATED,
        Json(CreateAgentRunResponse { data: run }),
    ))
}

pub async fn get_agent_run(
    request_user: RequestUser,
    State(state): State<AppState>,
    Path(run_id): Path<String>,
) -> Result<Json<AgentRunDetail>, ApiError> {
    require_admin(&state, &request_user).await?;
    Ok(Json(
        state.core().get_agent_run(parse_run_id(&run_id)?).await?,
    ))
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
