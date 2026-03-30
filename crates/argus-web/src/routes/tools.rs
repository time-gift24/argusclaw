use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Json;
use serde::Serialize;

use crate::error::ApiError;
use crate::state::AppState;

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/tools", get(list_tools))
        .route("/observability/thread-pool/snapshot", get(thread_pool_snapshot))
        .route("/observability/thread-pool/state", get(thread_pool_state))
}

#[derive(Debug, Clone, Serialize)]
pub struct ToolInfoPayload {
    pub name: String,
    pub description: String,
    pub risk_level: String,
    pub parameters: serde_json::Value,
}

async fn list_tools(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let tools = state.wing.list_tools().await;
    let payload: Vec<ToolInfoPayload> = tools
        .into_iter()
        .map(|t| ToolInfoPayload {
            name: t.name,
            description: t.description,
            risk_level: format!("{:?}", t.risk_level).to_lowercase(),
            parameters: t.parameters,
        })
        .collect();
    Ok(Json(payload))
}

async fn thread_pool_snapshot(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let snapshot = state.wing.thread_pool_snapshot();
    Ok(Json(snapshot))
}

async fn thread_pool_state(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let tp_state = state.wing.thread_pool_state();
    Ok(Json(tp_state))
}
