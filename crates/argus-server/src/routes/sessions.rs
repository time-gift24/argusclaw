//! Session management routes.

use axum::extract::{Path, State};
use axum::response::IntoResponse;
use serde::Deserialize;

use crate::http::error::ApiError;
use crate::routes::extract_user_principal;
use crate::state::AppState;

/// Request body for POST /api/sessions.
#[derive(Debug, Deserialize)]
pub struct CreateSessionRequest {
    pub name: String,
}

/// GET /api/sessions -- list user-owned sessions.
pub async fn list_sessions(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> axum::response::Response {
    let principal = match extract_user_principal(&state, &headers).await {
        Some(p) => p,
        None => return ApiError::Unauthorized.into_response(),
    };

    let Some(chat) = &state.chat_services else {
        return ApiError::Internal("chat services not configured".to_string()).into_response();
    };

    match chat.list_sessions(&principal).await {
        Ok(sessions) => (axum::http::StatusCode::OK, axum::Json(sessions)).into_response(),
        Err(e) => ApiError::from(e).into_response(),
    }
}

/// POST /api/sessions -- create a user-owned session.
pub async fn create_session(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    axum::Json(body): axum::Json<CreateSessionRequest>,
) -> axum::response::Response {
    let principal = match extract_user_principal(&state, &headers).await {
        Some(p) => p,
        None => return ApiError::Unauthorized.into_response(),
    };

    let Some(chat) = &state.chat_services else {
        return ApiError::Internal("chat services not configured".to_string()).into_response();
    };

    match chat.create_session(&principal, &body.name).await {
        Ok(session_id) => {
            let json = serde_json::json!({ "id": session_id.to_string() });
            (axum::http::StatusCode::OK, axum::Json(json)).into_response()
        }
        Err(e) => ApiError::from(e).into_response(),
    }
}

/// GET /api/sessions/:session_id/threads -- list threads in a session.
pub async fn list_threads(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(session_id): Path<String>,
) -> axum::response::Response {
    let principal = match extract_user_principal(&state, &headers).await {
        Some(p) => p,
        None => return ApiError::Unauthorized.into_response(),
    };

    let Some(chat) = &state.chat_services else {
        return ApiError::Internal("chat services not configured".to_string()).into_response();
    };

    let session_id = match argus_protocol::SessionId::parse(&session_id) {
        Ok(id) => id,
        Err(_) => return ApiError::BadRequest("invalid session id".to_string()).into_response(),
    };

    match chat.list_threads(&principal, session_id).await {
        Ok(threads) => (axum::http::StatusCode::OK, axum::Json(threads)).into_response(),
        Err(e) => ApiError::from(e).into_response(),
    }
}
