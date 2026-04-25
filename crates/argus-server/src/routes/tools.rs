use axum::Json;
use axum::extract::State;

use crate::app_state::AppState;
use crate::server_core::ToolRegistryItem;

pub async fn list_tools(State(state): State<AppState>) -> Json<Vec<ToolRegistryItem>> {
    Json(state.core().list_tools())
}
