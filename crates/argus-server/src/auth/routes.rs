//! Auth-related HTTP routes.
//!
//! Routes:
//! - GET  /auth/login          -> redirect to dev authorize page
//! - GET  /dev-oauth/authorize -> render test account form
//! - POST /dev-oauth/authorize -> issue code, redirect to callback
//! - GET  /auth/callback       -> exchange code, upsert user, set cookie
//! - POST /auth/logout         -> clear cookie
//! - GET  /api/me              -> return authenticated user JSON

use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use axum::Router;
use serde::Deserialize;

use super::dev_oauth::DevOAuth2Provider;
use super::session::{AuthSession, SESSION_COOKIE_NAME};
use crate::state::AppState;

/// Query parameters for GET /dev-oauth/authorize.
#[derive(Debug, Deserialize)]
pub struct AuthorizeParams {
    state: String,
}

/// Form data for POST /dev-oauth/authorize.
#[derive(Debug, Deserialize)]
pub struct AuthorizeForm {
    state: String,
    account: String,
    display_name: String,
}

/// Query parameters for GET /auth/callback.
#[derive(Debug, Deserialize)]
pub struct CallbackParams {
    code: String,
    /// CSRF state from the authorize flow. Validated in production providers.
    #[allow(dead_code)]
    state: String,
}

/// Build the auth + API routes router.
pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/login", get(login_handler))
        .route("/dev-oauth/authorize", get(authorize_form_handler).post(authorize_submit_handler))
        .route("/auth/callback", get(callback_handler))
        .route("/auth/logout", post(logout_handler))
        .route("/api/me", get(me_handler))
}

/// GET /auth/login -- redirect to the dev authorize page with a CSRF state.
async fn login_handler(
    State(state): State<AppState>,
) -> Result<Redirect, StatusCode> {
    let csrf_state = uuid::Uuid::now_v7().to_string();
    let url = state
        .auth_provider
        .authorize_url(&csrf_state, "/auth/callback".to_string())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    Ok(Redirect::temporary(&url))
}

/// GET /dev-oauth/authorize -- render a simple HTML form for test accounts.
async fn authorize_form_handler(
    Query(params): Query<AuthorizeParams>,
) -> Html<String> {
    let html = format!(
        r#"<!DOCTYPE html>
<html><head><title>Dev OAuth2 Authorize</title></head>
<body>
<h1>Dev OAuth2 - Choose Test Account</h1>
<form method="POST" action="/dev-oauth/authorize">
  <input type="hidden" name="state" value="{state}" />
  <label>Account: <input type="text" name="account" value="dev@test.com" /></label><br/>
  <label>Display Name: <input type="text" name="display_name" value="Dev User" /></label><br/>
  <button type="submit">Authorize</button>
</form>
</body></html>"#,
        state = params.state
    );
    Html(html)
}

/// POST /dev-oauth/authorize -- generate a code, redirect to callback.
async fn authorize_submit_handler(
    State(state): State<AppState>,
    axum::Form(form): axum::Form<AuthorizeForm>,
) -> Result<Redirect, StatusCode> {
    let dev_provider = state
        .auth_provider
        .as_any()
        .downcast_ref::<DevOAuth2Provider>()
        .ok_or(StatusCode::INTERNAL_SERVER_ERROR)?;

    let identity = argus_protocol::OAuth2Identity {
        external_subject: format!("dev-oauth2|{}", form.account),
        account: form.account.clone(),
        display_name: form.display_name,
    };

    let code = dev_provider.issue_code(identity);
    let callback_url = format!("/auth/callback?code={code}&state={}", form.state);
    Ok(Redirect::temporary(&callback_url))
}

/// GET /auth/callback -- exchange code, upsert user, set session cookie.
async fn callback_handler(
    State(state): State<AppState>,
    Query(params): Query<CallbackParams>,
) -> Result<Response, StatusCode> {
    let identity = state
        .auth_provider
        .exchange_code(&params.code, "/auth/callback".to_string())
        .await
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

    let user = state
        .user_repo
        .upsert_from_oauth2(&identity)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let cookie_value = state.auth_session.create_session(user.id);

    let response = Redirect::temporary("/");
    let mut response = response.into_response();
    let cookie = format!(
        "{SESSION_COOKIE_NAME}={cookie_value}; Path=/; HttpOnly; SameSite=Lax"
    );
    response
        .headers_mut()
        .insert(axum::http::header::SET_COOKIE, cookie.parse().map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?);
    Ok(response)
}

/// POST /auth/logout -- clear the session cookie.
async fn logout_handler() -> Response {
    let mut response = Redirect::temporary("/").into_response();
    let cookie = format!(
        "{SESSION_COOKIE_NAME}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0"
    );
    response
        .headers_mut()
        .insert(axum::http::header::SET_COOKIE, cookie.parse().unwrap());
    response
}

/// GET /api/me -- return the authenticated user.
async fn me_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
) -> Response {
    let user_id = extract_user_id(&headers, &state.auth_session);
    let Some(user_id) = user_id else {
        return (StatusCode::UNAUTHORIZED, "not authenticated").into_response();
    };

    match state.user_repo.get_by_id(user_id).await {
        Ok(Some(user)) => {
            let body = serde_json::json!({
                "id": user.id,
                "account": user.account,
                "display_name": user.display_name,
            });
            (StatusCode::OK, axum::Json(body)).into_response()
        }
        Ok(None) => (StatusCode::UNAUTHORIZED, "user not found").into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "database error").into_response(),
    }
}

/// Extract the user ID from the session cookie in request headers.
fn extract_user_id(headers: &axum::http::HeaderMap, session: &AuthSession) -> Option<i64> {
    let cookie_header = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    // Parse cookies manually -- simple key=value pairs
    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some(value) = cookie.strip_prefix(&format!("{SESSION_COOKIE_NAME}=")) {
            return session.verify_session(value);
        }
    }
    None
}
