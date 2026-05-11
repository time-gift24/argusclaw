pub mod account;
pub mod agent_runs;
pub mod auth;
pub mod bootstrap;
pub mod chat;
pub mod health;
pub mod mcp;
pub mod providers;
pub mod runtime;
pub mod templates;
pub mod tools;

use axum::routing::{get, post};
use axum::{Router, http::StatusCode, routing::any, routing::patch};

use crate::app_state::AppState;
use crate::error::ApiError;
use crate::user_context::RequestUser;

pub(crate) async fn require_admin(
    state: &AppState,
    request_user: &RequestUser,
) -> Result<(), ApiError> {
    if state.core().is_request_user_admin(request_user).await? {
        Ok(())
    } else {
        Err(ApiError::forbidden("admin access is required"))
    }
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/login", get(auth::login))
        .route("/auth/dev-login", get(auth::dev_login))
        .route("/auth/callback", get(auth::callback))
        .route("/auth/logout", get(auth::logout))
        .route("/api/v1/health", get(health::get_health))
        .route("/api/v1/auth/me", get(auth::me))
        .route(
            "/api/v1/account",
            get(account::get_account).put(account::configure_account),
        )
        .route("/api/v1/bootstrap", get(bootstrap::get_bootstrap))
        .route("/api/v1/runtime", get(runtime::get_runtime_state))
        .route("/api/v1/runtime/events", get(runtime::runtime_events))
        .route("/api/v1/tools", get(tools::list_tools))
        .route("/api/v1/agents/runs", post(agent_runs::create_agent_run))
        .route(
            "/api/v1/agents/runs/{run_id}",
            get(agent_runs::get_agent_run),
        )
        .route("/api/v1/chat/options", get(chat::get_chat_options))
        .route(
            "/api/v1/chat/sessions",
            get(chat::list_sessions).post(chat::create_session),
        )
        .route(
            "/api/v1/chat/sessions/with-thread",
            post(chat::create_session_with_thread),
        )
        .route(
            "/api/v1/chat/sessions/{session_id}",
            patch(chat::rename_session).delete(chat::delete_session),
        )
        .route(
            "/api/v1/chat/sessions/{session_id}/threads",
            get(chat::list_threads).post(chat::create_thread),
        )
        .route(
            "/api/v1/chat/sessions/{session_id}/threads/{thread_id}",
            get(chat::get_thread_snapshot)
                .patch(chat::rename_thread)
                .delete(chat::delete_thread),
        )
        .route(
            "/api/v1/chat/sessions/{session_id}/threads/{thread_id}/model",
            patch(chat::update_thread_model),
        )
        .route(
            "/api/v1/chat/sessions/{session_id}/threads/{thread_id}/activate",
            post(chat::activate_thread),
        )
        .route(
            "/api/v1/chat/sessions/{session_id}/threads/{thread_id}/messages",
            get(chat::list_messages).post(chat::send_message),
        )
        .route(
            "/api/v1/chat/sessions/{session_id}/threads/{thread_id}/jobs",
            get(chat::list_thread_jobs),
        )
        .route("/api/v1/chat/jobs/{job_id}", get(chat::get_chat_job))
        .route(
            "/api/v1/chat/sessions/{session_id}/threads/{thread_id}/cancel",
            post(chat::cancel_thread),
        )
        .route(
            "/api/v1/chat/sessions/{session_id}/threads/{thread_id}/events",
            get(chat::thread_events),
        )
        .route(
            "/api/v1/providers",
            get(providers::list_providers).post(providers::create_provider),
        )
        .route(
            "/api/v1/providers/test",
            post(providers::test_provider_record),
        )
        .route(
            "/api/v1/providers/{provider_id}/test",
            post(providers::test_provider_connection),
        )
        .route(
            "/api/v1/providers/{provider_id}",
            patch(providers::update_provider).delete(providers::delete_provider),
        )
        .route(
            "/api/v1/agents/templates",
            get(templates::list_templates).post(templates::create_template),
        )
        .route(
            "/api/v1/agents/templates/{template_id}",
            patch(templates::update_template).delete(templates::delete_template),
        )
        .route(
            "/api/v1/mcp/servers",
            get(mcp::list_mcp_servers).post(mcp::create_mcp_server),
        )
        .route("/api/v1/mcp/servers/test", post(mcp::test_mcp_server_input))
        .route(
            "/api/v1/mcp/servers/{server_id}/test",
            post(mcp::test_mcp_server_connection),
        )
        .route(
            "/api/v1/mcp/servers/{server_id}/tools",
            get(mcp::list_mcp_server_tools),
        )
        .route(
            "/api/v1/mcp/servers/{server_id}",
            patch(mcp::update_mcp_server).delete(mcp::delete_mcp_server),
        )
        .route("/api/v1", any(api_not_found))
        .route("/api/v1/{*path}", any(api_not_found))
}

async fn api_not_found() -> StatusCode {
    StatusCode::NOT_FOUND
}
