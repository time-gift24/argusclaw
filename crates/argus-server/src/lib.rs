pub mod app_state;
mod db;
pub mod error;
mod resolver;
pub mod response;
pub mod routes;
pub mod server_config;
pub mod server_core;

use app_state::AppState;
use argus_repository::migrate;
use axum::Router;
use server_core::ServerCore;

pub fn router(state: AppState) -> Router {
    routes::router().with_state(state)
}

pub async fn build_app(database_path: Option<&str>) -> argus_protocol::Result<Router> {
    let core = ServerCore::init(database_path).await?;
    Ok(router(AppState::new(core)))
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
