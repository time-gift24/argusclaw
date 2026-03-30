use axum::extract::{Path, State};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::IntoResponse;
use axum::routing::get;
use std::convert::Infallible;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt as _;

use argus_protocol::{SessionId, ThreadEventEnvelope, ThreadId};

use crate::error::ApiError;
use crate::state::AppState;

pub fn router() -> axum::Router<AppState> {
    axum::Router::new().route(
        "/sessions/{session_id}/threads/{thread_id}/events",
        get(stream_events),
    )
}

async fn stream_events(
    State(state): State<AppState>,
    Path((session_id_str, thread_id_str)): Path<(String, String)>,
) -> Result<impl IntoResponse, ApiError> {
    let session_id =
        SessionId::parse(&session_id_str).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let thread_id =
        ThreadId::parse(&thread_id_str).map_err(|e| ApiError::BadRequest(e.to_string()))?;

    let receiver = state
        .wing
        .subscribe(session_id, thread_id)
        .await
        .ok_or_else(|| ApiError::NotFound("Thread not found or not active".into()))?;

    let sid = session_id_str;
    let stream = BroadcastStream::new(receiver).filter_map(move |result| {
        match result {
            Ok(event) => {
                let envelope = ThreadEventEnvelope::from_thread_event(
                    sid.clone(),
                    event,
                )?;
                let json = serde_json::to_string(&envelope).ok()?;
                Some(Ok::<_, Infallible>(Event::default().event("thread-event").data(json)))
            }
            Err(_) => None,
        }
    });

    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}
