use axum::Json;
use axum::extract::State;

use crate::app_state::AppState;
use crate::error::ApiError;
use crate::server_core::ToolRegistryItem;
use crate::user_context::RequestUser;

use super::require_admin;

pub async fn list_tools(
    request_user: RequestUser,
    State(state): State<AppState>,
) -> Result<Json<Vec<ToolRegistryItem>>, ApiError> {
    require_admin(&state, &request_user).await?;
    Ok(Json(state.core().list_tools()))
}
