use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};

use crate::app_state::AppState;
use crate::error::ApiError;

#[derive(Debug, Serialize, Deserialize)]
pub struct BootstrapResponse {
    pub instance_name: String,
    pub provider_count: usize,
    pub template_count: usize,
    pub mcp_server_count: usize,
    pub default_provider_id: i64,
    pub default_template_id: Option<i64>,
    pub mcp_ready_count: usize,
}

pub async fn get_bootstrap(
    State(state): State<AppState>,
) -> Result<Json<BootstrapResponse>, ApiError> {
    let providers = state.core().list_providers().await?;
    let templates = state.core().list_templates().await?;
    let mcp_servers = state.core().list_mcp_servers().await?;
    let settings = state.core().admin_settings().await;
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

    Ok(Json(BootstrapResponse {
        instance_name: settings.instance_name,
        provider_count: providers.len(),
        template_count: templates.len(),
        mcp_server_count: mcp_servers.len(),
        default_provider_id: default_provider.id.into_inner(),
        default_template_id,
        mcp_ready_count,
    }))
}
