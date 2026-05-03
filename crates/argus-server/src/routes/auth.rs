use axum::extract::{Query, State};
use axum::http::header::{LOCATION, SET_COOKIE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};

use crate::app_state::AppState;
use crate::auth::{self, AuthenticatedUser};
use crate::error::ApiError;

#[derive(Debug, Deserialize)]
pub struct LoginQuery {
    next: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct CallbackQuery {
    code: String,
    state: String,
}

#[derive(Debug, Deserialize)]
pub struct LogoutQuery {
    redirect: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AuthMeResponse {
    uuid: String,
}

pub async fn login(
    State(state): State<AppState>,
    Query(query): Query<LoginQuery>,
) -> Result<Response, ApiError> {
    let (location, state_cookie) = state.auth().login_location(query.next.as_deref()).await?;
    redirect_with_cookies(&location, [state_cookie])
}

pub async fn callback(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<CallbackQuery>,
) -> Result<Response, ApiError> {
    let (session_cookie, next) = state
        .auth()
        .complete_authorization(&query.code, &query.state, &headers)
        .await?;
    redirect_with_cookies(
        &next,
        [session_cookie, state.auth().clear_login_state_cookie()],
    )
}

pub async fn logout(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(query): Query<LogoutQuery>,
) -> Result<Response, ApiError> {
    let clear_session = state.auth().clear_session(&headers).await;
    let location = state.auth().logout_location(query.redirect.as_deref());
    redirect_with_cookies(&location, [clear_session])
}

pub async fn me(
    user: Option<Extension<AuthenticatedUser>>,
) -> Result<Json<AuthMeResponse>, ApiError> {
    let Extension(user) = user.ok_or_else(|| ApiError::unauthorized("authentication required"))?;
    Ok(Json(AuthMeResponse {
        uuid: user.external_id().to_string(),
    }))
}

fn redirect_with_cookies<const N: usize>(
    location: &str,
    cookies: [String; N],
) -> Result<Response, ApiError> {
    let mut response = StatusCode::SEE_OTHER.into_response();
    response
        .headers_mut()
        .insert(LOCATION, auth::header_value(location)?);
    for cookie in cookies {
        response
            .headers_mut()
            .append(SET_COOKIE, auth::header_value(&cookie)?);
    }
    Ok(response)
}
