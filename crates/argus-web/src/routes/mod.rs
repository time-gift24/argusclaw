use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::state::AppState;

mod account;
mod approvals;
mod events;
mod messaging;
mod providers;
mod sessions;
mod templates;
mod tools;

pub fn build_router(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    Router::new()
        .nest("/api", api_routes())
        .with_state(state)
        .layer(cors)
        .layer(TraceLayer::new_for_http())
}

fn api_routes() -> Router<AppState> {
    Router::new()
        .merge(providers::router())
        .merge(templates::router())
        .merge(sessions::router())
        .merge(messaging::router())
        .merge(approvals::router())
        .merge(tools::router())
        .merge(account::router())
        .merge(events::router())
}
