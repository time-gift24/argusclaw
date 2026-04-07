//! Agent listing route.

use axum::response::IntoResponse;

use crate::http::error::ApiError;
use crate::routes::extract_user_principal;
use crate::state::AppState;

/// GET /api/agents -- list enabled agents.
pub async fn list_agents(
    axum::extract::State(state): axum::extract::State<AppState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    if extract_user_principal(&state, &headers).await.is_none() {
        return ApiError::Unauthorized.into_response();
    }

    let agents = state.chat_services.list_enabled_agents().await;
    (axum::http::StatusCode::OK, axum::Json(agents)).into_response()
}
