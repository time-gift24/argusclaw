use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};

use argus_protocol::LlmProviderId;

use crate::app_state::AppState;
use crate::error::ApiError;
use crate::response::MutationResponse;
use crate::server_core::AdminSettings;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SettingsResponse {
    pub instance_name: String,
    pub default_provider_id: i64,
    pub default_provider_name: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UpdateSettingsRequest {
    pub instance_name: String,
    pub default_provider_id: i64,
}

pub async fn get_settings(
    State(state): State<AppState>,
) -> Result<Json<SettingsResponse>, ApiError> {
    Ok(Json(read_settings(&state).await?))
}

pub async fn update_settings(
    State(state): State<AppState>,
    Json(request): Json<UpdateSettingsRequest>,
) -> Result<Json<MutationResponse<SettingsResponse>>, ApiError> {
    state
        .core()
        .set_default_provider(LlmProviderId::new(request.default_provider_id))
        .await?;
    state
        .core()
        .update_admin_settings(AdminSettings {
            instance_name: request.instance_name,
        })
        .await?;

    Ok(Json(MutationResponse::new(read_settings(&state).await?)))
}

async fn read_settings(state: &AppState) -> Result<SettingsResponse, ApiError> {
    let settings = state.core().admin_settings().await;
    let default_provider = state.core().get_default_provider_record().await?;

    Ok(SettingsResponse {
        instance_name: settings.instance_name,
        default_provider_id: default_provider.id.into_inner(),
        default_provider_name: default_provider.display_name,
    })
}
