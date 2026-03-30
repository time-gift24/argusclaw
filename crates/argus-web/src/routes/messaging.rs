use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::routing::post;
use axum::Json;
use argus_protocol::{SessionId, ThreadId};

use crate::error::ApiError;
use crate::state::AppState;

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route(
            "/sessions/{session_id}/threads/{thread_id}/send",
            post(send_message),
        )
        .route(
            "/sessions/{session_id}/threads/{thread_id}/cancel",
            post(cancel_turn),
        )
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct SendMessageBody {
    pub content: String,
}

async fn send_message(
    State(state): State<AppState>,
    Path((session_id, thread_id)): Path<(String, String)>,
    Json(body): Json<SendMessageBody>,
) -> Result<impl IntoResponse, ApiError> {
    let session_id =
        SessionId::parse(&session_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let thread_id =
        ThreadId::parse(&thread_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    state
        .wing
        .send_message(session_id, thread_id, body.content)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({ "success": true })))
}

async fn cancel_turn(
    State(state): State<AppState>,
    Path((session_id, thread_id)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    let session_id =
        SessionId::parse(&session_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let thread_id =
        ThreadId::parse(&thread_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    state
        .wing
        .cancel_turn(session_id, thread_id)
        .await
        .map_err(ApiError::from)?;
    Ok(Json(serde_json::json!({ "success": true })))
}
