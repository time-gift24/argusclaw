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
    let principal = match extract_user_principal(&state, &headers).await {
        Some(p) => p,
        None => return ApiError::Unauthorized.into_response(),
    };
    drop(principal);

    let Some(chat) = &state.chat_services else {
        return ApiError::Internal("chat services not configured".to_string()).into_response();
    };

    let agents = chat.list_enabled_agents().await;
    (axum::http::StatusCode::OK, axum::Json(agents)).into_response()
}
