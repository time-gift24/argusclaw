use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};

use super::require_admin;
use crate::app_state::AppState;
use crate::error::ApiError;
use crate::user_context::RequestUser;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AccountStatusResponse {
    pub configured: bool,
    pub username: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ConfigureAccountRequest {
    pub username: String,
    pub password: String,
}

pub async fn get_account(
    request_user: RequestUser,
    State(state): State<AppState>,
) -> Result<Json<AccountStatusResponse>, ApiError> {
    require_admin(&state, &request_user).await?;
    account_status(&state).await.map(Json)
}

pub async fn configure_account(
    request_user: RequestUser,
    State(state): State<AppState>,
    Json(request): Json<ConfigureAccountRequest>,
) -> Result<Json<AccountStatusResponse>, ApiError> {
    require_admin(&state, &request_user).await?;
    let username = request.username.trim();
    if username.is_empty() {
        return Err(ApiError::bad_request("username is required"));
    }

    if request.password.trim().is_empty() {
        return Err(ApiError::bad_request("password is required"));
    }

    state
        .core()
        .configure_account(username, &request.password)
        .await?;
    account_status(&state).await.map(Json)
}

async fn account_status(state: &AppState) -> Result<AccountStatusResponse, ApiError> {
    let username = state.core().get_account_username().await?;
    Ok(AccountStatusResponse {
        configured: username.is_some(),
        username,
    })
}
