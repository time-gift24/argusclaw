//! SSE event streaming route.

use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::response::sse::{Event, Sse};
use serde::Deserialize;
use std::convert::Infallible;
use tokio_stream::StreamExt;

use crate::http::error::ApiError;
use crate::routes::extract_user_principal;
use crate::state::AppState;

/// Query parameters for GET /api/threads/:thread_id/events.
#[derive(Debug, Deserialize)]
pub struct EventsQuery {
    pub session_id: String,
}

/// GET /api/threads/:thread_id/events -- SSE event stream for a thread.
pub async fn stream_events(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Path(thread_id): Path<String>,
    axum::extract::Query(query): axum::extract::Query<EventsQuery>,
) -> axum::response::Response {
    let principal = match extract_user_principal(&state, &headers).await {
        Some(p) => p,
        None => return ApiError::Unauthorized.into_response(),
    };

    let Some(chat) = &state.chat_services else {
        return ApiError::Internal("chat services not configured".to_string()).into_response();
    };

    let session_id = match argus_protocol::SessionId::parse(&query.session_id) {
        Ok(id) => id,
        Err(_) => return ApiError::BadRequest("invalid session id".to_string()).into_response(),
    };

    let thread_id = match argus_protocol::ThreadId::parse(&thread_id) {
        Ok(id) => id,
        Err(_) => return ApiError::BadRequest("invalid thread id".to_string()).into_response(),
    };

    let receiver = match chat.subscribe(&principal, session_id, thread_id).await {
        Some(rx) => rx,
        None => return ApiError::NotFound("thread not found or access denied".to_string()).into_response(),
    };

    let stream = tokio_stream::wrappers::BroadcastStream::new(receiver);
    let event_stream = stream.filter_map(|result| {
        match result {
            Ok(event) => {
                let data = serde_json::to_string(&event).unwrap_or_default();
                Some(Ok::<_, Infallible>(Event::default().data(data)))
            }
            Err(_) => None,
        }
    });

    let sse = Sse::new(event_stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(30)),
    );

    sse.into_response()
}
