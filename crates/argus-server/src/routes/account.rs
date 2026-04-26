use axum::Json;
use axum::extract::State;
use serde::{Deserialize, Serialize};

use crate::app_state::AppState;
use crate::error::ApiError;

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
    State(state): State<AppState>,
) -> Result<Json<AccountStatusResponse>, ApiError> {
    account_status(&state).await.map(Json)
}

pub async fn configure_account(
    State(state): State<AppState>,
    Json(request): Json<ConfigureAccountRequest>,
) -> Result<Json<AccountStatusResponse>, ApiError> {
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
