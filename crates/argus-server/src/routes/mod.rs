pub mod bootstrap;
pub mod health;
pub mod mcp;
pub mod providers;
pub mod settings;
pub mod templates;

use axum::routing::get;
use axum::{Router, routing::patch};

use crate::app_state::AppState;

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/api/v1/health", get(health::get_health))
        .route("/api/v1/bootstrap", get(bootstrap::get_bootstrap))
        .route(
            "/api/v1/settings",
            get(settings::get_settings).put(settings::update_settings),
        )
        .route(
            "/api/v1/providers",
            get(providers::list_providers).post(providers::create_provider),
        )
        .route(
            "/api/v1/providers/{provider_id}",
            patch(providers::update_provider),
        )
        .route(
            "/api/v1/agents/templates",
            get(templates::list_templates).post(templates::create_template),
        )
        .route(
            "/api/v1/agents/templates/{template_id}",
            patch(templates::update_template),
        )
        .route(
            "/api/v1/mcp/servers",
            get(mcp::list_mcp_servers).post(mcp::create_mcp_server),
        )
        .route(
            "/api/v1/mcp/servers/{server_id}",
            patch(mcp::update_mcp_server),
        )
}
