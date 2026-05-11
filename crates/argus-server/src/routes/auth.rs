use axum::extract::{Query, State};
use axum::http::header::{LOCATION, SET_COOKIE};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{Html, IntoResponse, Response};
use axum::{Extension, Json};
use serde::{Deserialize, Serialize};
use url::form_urlencoded;

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

#[derive(Debug, Deserialize)]
pub struct DevLoginQuery {
    role: String,
    next: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct AuthMeResponse {
    uuid: String,
}

const DEV_ADMIN_ID: &str = "dev-admin";
const DEV_ADMIN_NAME: &str = "本地管理员";
const DEV_USER_ID: &str = "dev-user";
const DEV_USER_NAME: &str = "本地普通用户";

pub async fn login(
    State(state): State<AppState>,
    Query(query): Query<LoginQuery>,
) -> Result<Response, ApiError> {
    if state.auth().is_dev_enabled() {
        return Ok(dev_login_page(query.next.as_deref()).into_response());
    }
    let (location, state_cookie) = state.auth().login_location(query.next.as_deref()).await?;
    redirect_with_cookies(&location, [state_cookie])
}

pub async fn dev_login(
    State(state): State<AppState>,
    Query(query): Query<DevLoginQuery>,
) -> Result<Response, ApiError> {
    if !state.auth().is_dev_enabled() {
        return Err(ApiError::bad_request("dev auth is not enabled"));
    }
    let (external_id, display_name, is_admin) = match query.role.as_str() {
        "admin" => (DEV_ADMIN_ID, DEV_ADMIN_NAME, true),
        "user" => (DEV_USER_ID, DEV_USER_NAME, false),
        _ => return Err(ApiError::bad_request("role must be admin or user")),
    };
    state
        .core()
        .set_dev_user_admin(external_id, Some(display_name), is_admin)
        .await?;
    let session_cookie = state
        .auth()
        .create_dev_session(external_id, Some(display_name))
        .await?;
    redirect_with_cookies(&sanitize_next(query.next.as_deref()), [session_cookie])
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

fn dev_login_page(next: Option<&str>) -> Html<String> {
    let next = sanitize_next(next);
    let encoded_next = form_urlencoded::byte_serialize(next.as_bytes()).collect::<String>();
    Html(format!(
        r#"<!doctype html>
<html lang="zh-CN">
<head>
  <meta charset="utf-8">
  <meta name="viewport" content="width=device-width, initial-scale=1">
  <title>ArgusWing 本地登录</title>
  <style>
    body {{ margin: 0; min-height: 100vh; display: grid; place-items: center; font-family: -apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif; background: #f6f8fb; color: #1f2937; }}
    main {{ width: min(420px, calc(100vw - 32px)); padding: 32px; border: 1px solid #dce3ee; border-radius: 12px; background: #fff; box-shadow: 0 16px 50px rgba(15, 23, 42, 0.08); }}
    h1 {{ margin: 0 0 8px; font-size: 24px; }}
    p {{ margin: 0 0 24px; color: #64748b; line-height: 1.7; }}
    a {{ display: block; padding: 12px 16px; border-radius: 8px; text-align: center; text-decoration: none; font-weight: 650; }}
    a + a {{ margin-top: 12px; }}
    .admin {{ color: #fff; background: #2563eb; }}
    .user {{ color: #1f2937; background: #eef2f7; }}
  </style>
</head>
<body>
  <main>
    <h1>ArgusWing 本地登录</h1>
    <p>请选择本地开发测试身份。这个入口只在 [auth].dev_enabled=true 时可用。</p>
    <a class="admin" href="/auth/dev-login?role=admin&next={encoded_next}">管理员登录</a>
    <a class="user" href="/auth/dev-login?role=user&next={encoded_next}">普通用户登录</a>
  </main>
</body>
</html>"#
    ))
}

fn sanitize_next(next: Option<&str>) -> String {
    match next.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if value.starts_with('/') && !value.starts_with("//") => value.to_string(),
        _ => "/".to_string(),
    }
}
