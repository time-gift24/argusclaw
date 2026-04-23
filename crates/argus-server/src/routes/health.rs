use axum::Json;
use serde::Serialize;

#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
}

pub async fn get_health() -> Json<HealthResponse> {
    Json(HealthResponse { status: "ok" })
}
