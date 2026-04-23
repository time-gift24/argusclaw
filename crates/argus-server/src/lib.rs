pub mod app_state;
pub mod error;
pub mod response;
pub mod routes;

use app_state::AppState;
use argus_repository::migrate;
use argus_wing::ArgusWing;
use axum::Router;

pub fn router(state: AppState) -> Router {
    routes::router().with_state(state)
}

pub async fn build_app(database_path: Option<&str>) -> argus_protocol::Result<Router> {
    let wing = ArgusWing::init(database_path).await?;
    Ok(router(AppState::new(wing)))
}

pub async fn router_for_test() -> Router {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("sqlite pool should connect for tests");
    migrate(&pool)
        .await
        .expect("test migrations should succeed");
    let wing = ArgusWing::with_pool(pool);
    router(AppState::new(wing))
}
