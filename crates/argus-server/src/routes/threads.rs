//! Thread message and cancel routes.

use axum::extract::{Path, State};
use axum::response::IntoResponse;
use serde::Deserialize;

use crate::http::error::ApiError;
use crate::routes::extract_user_principal;
use crate::state::AppState;

/// Request body for POST /api/threads/:thread_id/messages.
#[derive(Debug, Deserialize)]
pub struct SendMessageRequest {
    /// Session ID that owns the thread.
    pub session_id: String,
    /// Message content.
    pub content: String,
}

/// POST /api/threads/:thread_id/messages -- send a message to a thread.
pub async fn send_message(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(thread_id): Path<String>,
    axum::Json(body): axum::Json<SendMessageRequest>,
) -> axum::response::Response {
    let principal = match extract_user_principal(&state, &headers).await {
        Some(p) => p,
        None => return ApiError::Unauthorized.into_response(),
    };

    let Some(chat) = &state.chat_services else {
        return ApiError::Internal("chat services not configured".to_string()).into_response();
    };

    let session_id = match argus_protocol::SessionId::parse(&body.session_id) {
        Ok(id) => id,
        Err(_) => return ApiError::BadRequest("invalid session id".to_string()).into_response(),
    };

    let thread_id = match argus_protocol::ThreadId::parse(&thread_id) {
        Ok(id) => id,
        Err(_) => return ApiError::BadRequest("invalid thread id".to_string()).into_response(),
    };

    match chat
        .send_message(&principal, session_id, thread_id, body.content)
        .await
    {
        Ok(()) => (axum::http::StatusCode::OK, axum::Json(serde_json::json!({}))).into_response(),
        Err(e) => ApiError::from(e).into_response(),
    }
}
