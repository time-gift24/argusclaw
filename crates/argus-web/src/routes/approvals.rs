use axum::extract::{Path, State};
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Json;
use argus_protocol::ApprovalDecision;

use crate::error::ApiError;
use crate::state::AppState;

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/approvals/pending", get(list_pending_approvals))
        .route("/approvals/{request_id}/resolve", post(resolve_approval))
}

async fn list_pending_approvals(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let pending = state.wing.list_pending_approvals();
    Ok(Json(pending))
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ResolveApprovalBody {
    pub decision: String,
    pub resolved_by: Option<String>,
}

async fn resolve_approval(
    State(state): State<AppState>,
    Path(request_id): Path<String>,
    Json(body): Json<ResolveApprovalBody>,
) -> Result<impl IntoResponse, ApiError> {
    let request_id =
        uuid::Uuid::parse_str(&request_id).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let decision = match body.decision.as_str() {
        "approved" => ApprovalDecision::Approved,
        "denied" => ApprovalDecision::Denied,
        _ => {
            return Err(ApiError::BadRequest(format!(
                "Invalid approval decision: {}",
                body.decision
            )))
        }
    };

    state
        .wing
        .resolve_approval(request_id, decision, body.resolved_by)
        .map_err(ApiError::from)?;

    Ok(Json(serde_json::json!({ "success": true })))
}
