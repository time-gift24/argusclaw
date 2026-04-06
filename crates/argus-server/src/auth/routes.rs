//! Auth-related HTTP routes.

use axum::Router;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::{Html, IntoResponse, Redirect, Response};
use axum::routing::{get, post};
use serde::Deserialize;

use super::dev_oauth::DevOAuth2Provider;
use super::session::{AuthSession, OAUTH_STATE_COOKIE_NAME, SESSION_COOKIE_NAME};
use crate::state::AppState;

#[derive(Debug, Deserialize)]
pub struct AuthorizeParams {
    state: String,
}

#[derive(Debug, Deserialize)]
pub struct AuthorizeForm {
    state: String,
    account: String,
    display_name: String,
}

#[derive(Debug, Deserialize)]
pub struct CallbackParams {
    code: String,
    state: String,
}

pub fn router() -> Router<AppState> {
    Router::new()
        .route("/auth/login", get(login_handler))
        .route(
            "/dev-oauth/authorize",
            get(authorize_form_handler).post(authorize_submit_handler),
        )
        .route("/auth/callback", get(callback_handler))
        .route("/auth/logout", post(logout_handler))
        .route("/api/me", get(me_handler))
}

async fn login_handler(State(state): State<AppState>) -> Result<Response, StatusCode> {
    let csrf_state = uuid::Uuid::now_v7().to_string();
    let url = state
        .auth_provider
        .authorize_url(&csrf_state, "/auth/callback".to_string())
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut response = Redirect::temporary(&url).into_response();
    let signed_state = state.auth_session.sign_value(&csrf_state);
    response.headers_mut().append(
        axum::http::header::SET_COOKIE,
        build_cookie(
            OAUTH_STATE_COOKIE_NAME,
            &signed_state,
            &state.config,
            Some(300),
        )
        .parse()
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    );
    Ok(response)
}

async fn authorize_form_handler(Query(params): Query<AuthorizeParams>) -> Html<String> {
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

async fn callback_handler(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Query(params): Query<CallbackParams>,
) -> Result<Response, StatusCode> {
    validate_oauth_state(&headers, &state.auth_session, &params.state)
        .map_err(|_| StatusCode::UNAUTHORIZED)?;

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
    let mut response = Redirect::temporary("/").into_response();
    response.headers_mut().append(
        axum::http::header::SET_COOKIE,
        build_cookie(SESSION_COOKIE_NAME, &cookie_value, &state.config, None)
            .parse()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    );
    response.headers_mut().append(
        axum::http::header::SET_COOKIE,
        clear_cookie(OAUTH_STATE_COOKIE_NAME, &state.config)
            .parse()
            .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?,
    );
    Ok(response)
}

async fn logout_handler(State(state): State<AppState>) -> Response {
    let mut response = Redirect::temporary("/").into_response();
    let session_cookie = clear_cookie(SESSION_COOKIE_NAME, &state.config);
    let state_cookie = clear_cookie(OAUTH_STATE_COOKIE_NAME, &state.config);
    response.headers_mut().append(
        axum::http::header::SET_COOKIE,
        session_cookie.parse().unwrap(),
    );
    response.headers_mut().append(
        axum::http::header::SET_COOKIE,
        state_cookie.parse().unwrap(),
    );
    response
}

async fn me_handler(State(state): State<AppState>, headers: axum::http::HeaderMap) -> Response {
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
        Ok(None) => (StatusCode::UNAUTHORIZED, "not authenticated").into_response(),
        Err(_) => (StatusCode::INTERNAL_SERVER_ERROR, "database error").into_response(),
    }
}

pub fn extract_user_id(headers: &axum::http::HeaderMap, session: &AuthSession) -> Option<i64> {
    let cookie_value = extract_cookie_value(headers, SESSION_COOKIE_NAME)?;
    session.verify_session(&cookie_value)
}

fn validate_oauth_state(
    headers: &axum::http::HeaderMap,
    session: &AuthSession,
    callback_state: &str,
) -> Result<(), ()> {
    let cookie_value = extract_cookie_value(headers, OAUTH_STATE_COOKIE_NAME).ok_or(())?;
    let stored_state = session.verify_value(&cookie_value).ok_or(())?;
    if stored_state == callback_state {
        Ok(())
    } else {
        Err(())
    }
}

pub fn extract_cookie_value(headers: &axum::http::HeaderMap, cookie_name: &str) -> Option<String> {
    let cookie_header = headers.get(axum::http::header::COOKIE)?.to_str().ok()?;
    for cookie in cookie_header.split(';') {
        let cookie = cookie.trim();
        if let Some(value) = cookie.strip_prefix(&format!("{cookie_name}=")) {
            return Some(value.to_string());
        }
    }
    None
}

fn build_cookie(
    name: &str,
    value: &str,
    config: &crate::config::ServerConfig,
    max_age_secs: Option<u32>,
) -> String {
    let mut cookie = format!("{name}={value}; Path=/; HttpOnly; SameSite=Lax");
    if config.secure_cookies {
        cookie.push_str("; Secure");
    }
    if let Some(max_age_secs) = max_age_secs {
        cookie.push_str(&format!("; Max-Age={max_age_secs}"));
    }
    cookie
}

fn clear_cookie(name: &str, config: &crate::config::ServerConfig) -> String {
    build_cookie(name, "", config, Some(0))
}
