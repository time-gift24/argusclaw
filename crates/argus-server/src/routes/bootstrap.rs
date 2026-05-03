use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};

use crate::app_state::AppState;
use crate::error::ApiError;
use crate::user_context::RequestUser;

#[derive(Debug, Serialize, Deserialize)]
pub struct CurrentUserResponse {
    pub id: Option<String>,
    pub external_id: String,
    pub display_name: Option<String>,
    pub is_admin: bool,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct BootstrapResponse {
    pub instance_name: String,
    pub provider_count: usize,
    pub template_count: usize,
    pub mcp_server_count: usize,
    pub default_provider_id: i64,
    pub default_template_id: Option<i64>,
    pub mcp_ready_count: usize,
    pub current_user: CurrentUserResponse,
}

pub async fn get_bootstrap(
    request_user: RequestUser,
    State(state): State<AppState>,
) -> Result<Json<BootstrapResponse>, ApiError> {
    let providers = state.core().list_providers().await?;
    let templates = state.core().list_templates().await?;
    let mcp_servers = state.core().list_mcp_servers().await?;
    let default_provider = state.core().get_default_provider_record().await?;
    let default_template_id = state
        .core()
        .get_default_template()
        .await?
        .map(|template| template.id.inner());
    let mcp_ready_count = mcp_servers
        .iter()
        .filter(|server| matches!(server.status, argus_protocol::McpServerStatus::Ready))
        .count();
    let current_user = state.core().current_user(&request_user).await?;

    Ok(Json(BootstrapResponse {
        instance_name: state.core().instance_name().to_string(),
        provider_count: providers.len(),
        template_count: templates.len(),
        mcp_server_count: mcp_servers.len(),
        default_provider_id: default_provider.id.into_inner(),
        default_template_id,
        mcp_ready_count,
        current_user: CurrentUserResponse {
            id: Some(current_user.id.to_string()),
            external_id: request_user.external_id().to_string(),
            display_name: request_user.display_name().map(str::to_string),
            is_admin: current_user.is_admin,
        },
    }))
}
