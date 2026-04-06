//! Chat API routes module.
//!
//! Provides HTTP handlers for user-facing chat operations.

pub mod agents;
pub mod events;
pub mod sessions;
pub mod threads;

use axum::routing::{get, post};
use axum::Router;

use crate::auth::session::SESSION_COOKIE_NAME;
use crate::state::AppState;
use argus_session::UserPrincipal;

use self::{
    agents::list_agents,
    events::stream_events,
    sessions::{create_session, list_sessions, list_threads},
    threads::send_message,
};

/// Build the chat API router.
///
/// All routes require authentication via the session cookie.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/agents", get(list_agents))
        .route("/api/sessions", get(list_sessions).post(create_session))
        .route(
            "/api/sessions/{session_id}/threads",
            get(list_threads),
        )
        .route(
            "/api/threads/{thread_id}/messages",
            post(send_message),
        )
        .route(
            "/api/threads/{thread_id}/events",
            get(stream_events),
        )
}

/// Extract a `UserPrincipal` from the session cookie in request headers.
///
/// Returns `None` if the cookie is missing, malformed, or the user does not exist.
pub async fn extract_user_principal(
    state: &AppState,
    headers: &axum::http::HeaderMap,
) -> Option<UserPrincipal> {
    let user_id = extract_user_id(headers, &state.auth_session)?;
    let user = state.user_repo.get_by_id(user_id).await.ok()??;
    Some(UserPrincipal {
        user_id: user.id,
        account: user.account,
        display_name: user.display_name,
    })
}

/// Extract the user ID from the session cookie in request headers.
fn extract_user_id(
    headers: &axum::http::HeaderMap,
    session: &crate::auth::session::AuthSession,
) -> Option<i64> {
    let cookie_header = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some(value) = cookie.strip_prefix(&format!("{SESSION_COOKIE_NAME}=")) {
            return session.verify_session(value);
        }
    }
    None
}
