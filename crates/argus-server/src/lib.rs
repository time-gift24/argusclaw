pub mod app_state;
pub mod auth;
mod db;
pub mod error;
mod resolver;
pub mod response;
pub mod routes;
pub mod server_config;
pub mod server_core;
mod user_context;

use std::path::PathBuf;

use app_state::AppState;
use argus_repository::migrate;
use axum::{Router, middleware};
use server_config::ServerConfig;
use server_core::ServerCore;
use tower_http::services::{ServeDir, ServeFile};

pub fn router(state: AppState) -> Router {
    router_with_web_dist(state, None)
}

pub fn router_with_web_dist(state: AppState, web_dist_dir: Option<PathBuf>) -> Router {
    let router = routes::router()
        .with_state(state.clone())
        .layer(middleware::from_fn_with_state(
            state,
            auth::require_api_auth,
        ));
    match web_dist_dir {
        Some(web_dist_dir) => router.fallback_service(
            ServeDir::new(&web_dist_dir).fallback(ServeFile::new(web_dist_dir.join("index.html"))),
        ),
        None => router,
    }
}

pub async fn build_app(database_url: Option<&str>) -> argus_protocol::Result<Router> {
    let config = ServerConfig::from_env().map_err(|error| argus_protocol::ArgusError::IoError {
        reason: format!("invalid server config: {error}"),
    })?;
    build_app_with_config(database_url, &config).await
}

pub async fn build_app_with_config(
    database_url: Option<&str>,
    config: &ServerConfig,
) -> argus_protocol::Result<Router> {
    let core = ServerCore::init(database_url).await?;
    let auth =
        auth::AuthState::from_env().map_err(|error| argus_protocol::ArgusError::IoError {
            reason: format!("invalid OAuth2 config: {error}"),
        })?;
    Ok(router_with_web_dist(
        AppState::with_auth(core, auth),
        config.web_dist_dir.clone(),
    ))
}

pub async fn router_for_test() -> Router {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("sqlite pool should connect for tests");
    migrate(&pool)
        .await
        .expect("test migrations should succeed");
    let core = ServerCore::with_pool(pool)
        .await
        .expect("server core should initialize for tests");
    router(AppState::new(core))
}

pub async fn router_for_test_with_web_dist(web_dist_dir: PathBuf) -> Router {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("sqlite pool should connect for tests");
    migrate(&pool)
        .await
        .expect("test migrations should succeed");
    let core = ServerCore::with_pool(pool)
        .await
        .expect("server core should initialize for tests");
    router_with_web_dist(AppState::new(core), Some(web_dist_dir))
}
