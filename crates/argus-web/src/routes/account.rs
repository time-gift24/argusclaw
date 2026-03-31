use axum::extract::State;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::error::ApiError;
use crate::state::AppState;

pub fn router() -> axum::Router<AppState> {
    axum::Router::new()
        .route("/account/me", get(get_current_user))
        .route("/account/has-user", get(has_account))
        .route("/account/setup", post(setup_account))
        .route("/account/login", post(login))
        .route("/account/logout", post(logout))
}

#[derive(Debug, Clone, Serialize)]
pub struct UserInfoPayload {
    pub username: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct HasUserPayload {
    pub has_user: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct LoginPayload {
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetupAccountBody {
    pub username: String,
    pub password: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoginBody {
    pub username: String,
    pub password: String,
}

async fn get_current_user(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let user = state
        .wing
        .account_manager()
        .get_current_user()
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    let payload = user.map(|u| UserInfoPayload {
        username: u.username,
    });
    Ok(Json(payload))
}

async fn has_account(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    let has_user = state
        .wing
        .account_manager()
        .has_account()
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(Json(HasUserPayload { has_user }))
}

async fn setup_account(
    State(state): State<AppState>,
    Json(body): Json<SetupAccountBody>,
) -> Result<impl IntoResponse, ApiError> {
    state
        .wing
        .account_manager()
        .setup_account(&body.username, &body.password)
        .await
        .map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(serde_json::json!({ "success": true })))
}

async fn login(
    State(state): State<AppState>,
    Json(body): Json<LoginBody>,
) -> Result<impl IntoResponse, ApiError> {
    let success = state
        .wing
        .account_manager()
        .login(&body.username, &body.password)
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(Json(LoginPayload { success }))
}

async fn logout(
    State(state): State<AppState>,
) -> Result<impl IntoResponse, ApiError> {
    state
        .wing
        .account_manager()
        .logout()
        .await
        .map_err(|e| ApiError::Internal(e.to_string()))?;
    Ok(Json(serde_json::json!({ "success": true })))
}
