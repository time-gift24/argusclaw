use std::collections::HashMap;
use std::time::Duration;

use axum::body::Body;
use axum::extract::State;
use axum::http::header;
use axum::http::{HeaderMap, HeaderValue, Request, Response};
use axum::middleware::Next;
use chrono::{DateTime, TimeDelta, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::RwLock;
use url::Url;
use uuid::Uuid;

use crate::app_state::AppState;
use crate::error::ApiError;

const DEFAULT_SCOPE: &str = "base.profile";
const SESSION_COOKIE_NAME: &str = "argus_session";
const LOGIN_STATE_COOKIE_NAME: &str = "argus_oauth_state";
const SESSION_TTL_SECONDS: i64 = 8 * 60 * 60;
const REFRESH_SKEW_SECONDS: i64 = 60;
const OAUTH_AUTHORIZE_PATH: &str = "/saaslogin1/oauth2/authorize";
const OAUTH_TOKEN_PATH: &str = "/saaslogin1/oauth2/accesstoken";
const OAUTH_USERINFO_PATH: &str = "/saaslogin1/oauth2/userinfo";
const OAUTH_LOGOUT_PATH: &str = "/saaslogin1/oauth2/logout";

#[derive(Debug, Clone)]
pub struct AuthConfig {
    pub enabled: bool,
    client_id: String,
    client_secret: String,
    authorize_url: Url,
    token_url: Url,
    userinfo_url: Url,
    logout_url: Option<Url>,
    redirect_uri: String,
    scope: String,
    cookie_secure: bool,
}

#[derive(Debug, Error)]
pub enum AuthConfigError {
    #[error("{0} is required when ARGUS_OAUTH_ENABLED=true")]
    MissingEnv(&'static str),
    #[error("{name} must be a valid URL: {source}")]
    InvalidUrl {
        name: &'static str,
        #[source]
        source: url::ParseError,
    },
}

struct AuthEnvValues<'a> {
    enabled: Option<&'a str>,
    client_id: Option<&'a str>,
    client_secret: Option<&'a str>,
    base_url: Option<&'a str>,
    authorize_url: Option<&'a str>,
    token_url: Option<&'a str>,
    userinfo_url: Option<&'a str>,
    logout_url: Option<&'a str>,
    redirect_uri: Option<&'a str>,
    scope: Option<&'a str>,
    cookie_secure: Option<&'a str>,
}

#[derive(Debug, Clone)]
pub struct AuthState {
    inner: std::sync::Arc<AuthStateInner>,
}

#[derive(Debug)]
struct AuthStateInner {
    config: Option<AuthConfig>,
    http: reqwest::Client,
    login_states: RwLock<HashMap<String, OAuthLoginState>>,
    sessions: RwLock<HashMap<String, AuthSession>>,
}

#[derive(Debug, Clone)]
struct OAuthLoginState {
    next: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Clone)]
struct AuthSession {
    user: AuthenticatedUser,
    access_token: Option<String>,
    refresh_token: Option<String>,
    expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuthenticatedUser {
    external_id: String,
    display_name: Option<String>,
}

impl AuthenticatedUser {
    #[must_use]
    pub fn new(external_id: String, display_name: Option<String>) -> Self {
        Self {
            external_id,
            display_name,
        }
    }

    #[must_use]
    pub fn external_id(&self) -> &str {
        &self.external_id
    }

    #[must_use]
    pub fn display_name(&self) -> Option<&str> {
        self.display_name.as_deref()
    }
}

#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<i64>,
}

#[derive(Debug, Deserialize)]
struct UserInfoResponse {
    uuid: String,
}

#[derive(Debug, Serialize)]
struct TokenRequest<'a> {
    client_id: &'a str,
    client_secret: &'a str,
    redirect_uri: &'a str,
    grant_type: &'static str,
    code: &'a str,
}

#[derive(Debug, Serialize)]
struct UserInfoRequest<'a> {
    client_id: &'a str,
    access_token: &'a str,
    scope: &'a str,
}

#[derive(Debug, Serialize)]
struct RefreshTokenRequest<'a> {
    client_id: &'a str,
    grant_type: &'static str,
    refresh_token: &'a str,
}

impl AuthConfig {
    pub fn from_env() -> Result<Option<Self>, AuthConfigError> {
        let enabled = std::env::var("ARGUS_OAUTH_ENABLED").ok();
        let client_id = std::env::var("ARGUS_OAUTH_CLIENT_ID").ok();
        let client_secret = std::env::var("ARGUS_OAUTH_CLIENT_SECRET").ok();
        let base_url = std::env::var("ARGUS_OAUTH_BASE_URL").ok();
        let authorize_url = std::env::var("ARGUS_OAUTH_AUTHORIZE_URL").ok();
        let token_url = std::env::var("ARGUS_OAUTH_TOKEN_URL").ok();
        let userinfo_url = std::env::var("ARGUS_OAUTH_USERINFO_URL").ok();
        let logout_url = std::env::var("ARGUS_OAUTH_LOGOUT_URL").ok();
        let redirect_uri = std::env::var("ARGUS_OAUTH_REDIRECT_URI").ok();
        let scope = std::env::var("ARGUS_OAUTH_SCOPE").ok();
        let cookie_secure = std::env::var("ARGUS_OAUTH_COOKIE_SECURE").ok();

        Self::from_env_values(AuthEnvValues {
            enabled: enabled.as_deref(),
            client_id: client_id.as_deref(),
            client_secret: client_secret.as_deref(),
            base_url: base_url.as_deref(),
            authorize_url: authorize_url.as_deref(),
            token_url: token_url.as_deref(),
            userinfo_url: userinfo_url.as_deref(),
            logout_url: logout_url.as_deref(),
            redirect_uri: redirect_uri.as_deref(),
            scope: scope.as_deref(),
            cookie_secure: cookie_secure.as_deref(),
        })
    }

    fn from_env_values(values: AuthEnvValues<'_>) -> Result<Option<Self>, AuthConfigError> {
        if !env_bool_value(values.enabled) {
            tracing::info!(
                oauth_enabled = false,
                oauth_base_url = env_value_for_log(values.base_url),
                oauth_client_id = env_value_for_log(values.client_id),
                oauth_client_secret_set = env_value_present(values.client_secret),
                oauth_redirect_uri = env_value_for_log(values.redirect_uri),
                oauth_scope = env_value_for_log(values.scope),
                oauth_cookie_secure = env_value_for_log(values.cookie_secure),
                "OAuth2 authentication disabled"
            );
            return Ok(None);
        }

        tracing::info!(
            oauth_enabled = true,
            oauth_base_url = env_value_for_log(values.base_url),
            oauth_client_id = env_value_for_log(values.client_id),
            oauth_client_secret_set = env_value_present(values.client_secret),
            oauth_authorize_url = env_value_for_log(values.authorize_url),
            oauth_token_url = env_value_for_log(values.token_url),
            oauth_userinfo_url = env_value_for_log(values.userinfo_url),
            oauth_logout_url = env_value_for_log(values.logout_url),
            oauth_redirect_uri = env_value_for_log(values.redirect_uri),
            oauth_scope = env_value_for_log(values.scope),
            oauth_cookie_secure = env_value_for_log(values.cookie_secure),
            "OAuth2 authentication environment detected"
        );

        let client_id = required_value("ARGUS_OAUTH_CLIENT_ID", values.client_id)?;
        let client_secret = required_value("ARGUS_OAUTH_CLIENT_SECRET", values.client_secret)?;
        let base_url = values
            .base_url
            .map(|value| parse_url("ARGUS_OAUTH_BASE_URL", value))
            .transpose()?;
        let authorize_url = endpoint_url(
            "ARGUS_OAUTH_AUTHORIZE_URL",
            values.authorize_url,
            base_url.as_ref(),
            OAUTH_AUTHORIZE_PATH,
        )?;
        let token_url = endpoint_url(
            "ARGUS_OAUTH_TOKEN_URL",
            values.token_url,
            base_url.as_ref(),
            OAUTH_TOKEN_PATH,
        )?;
        let userinfo_url = endpoint_url(
            "ARGUS_OAUTH_USERINFO_URL",
            values.userinfo_url,
            base_url.as_ref(),
            OAUTH_USERINFO_PATH,
        )?;
        let redirect_uri = required_value("ARGUS_OAUTH_REDIRECT_URI", values.redirect_uri)?;
        let scope = values
            .scope
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(DEFAULT_SCOPE)
            .to_string();
        let logout_url = optional_endpoint_url(
            "ARGUS_OAUTH_LOGOUT_URL",
            values.logout_url,
            base_url.as_ref(),
            OAUTH_LOGOUT_PATH,
        )?;
        let cookie_secure = values.cookie_secure.map(parse_bool).unwrap_or(true);

        tracing::info!(
            oauth_enabled = true,
            oauth_base_url = base_url
                .as_ref()
                .map(Url::as_str)
                .unwrap_or("<not set>"),
            oauth_client_id = %client_id,
            oauth_client_secret_set = env_value_present(values.client_secret),
            oauth_authorize_url = %authorize_url,
            oauth_authorize_url_override = env_value_present(values.authorize_url),
            oauth_token_url = %token_url,
            oauth_token_url_override = env_value_present(values.token_url),
            oauth_userinfo_url = %userinfo_url,
            oauth_userinfo_url_override = env_value_present(values.userinfo_url),
            oauth_logout_url = logout_url
                .as_ref()
                .map(Url::as_str)
                .unwrap_or("<not set>"),
            oauth_logout_url_override = env_value_present(values.logout_url),
            oauth_redirect_uri = %redirect_uri,
            oauth_scope = %scope,
            oauth_cookie_secure = cookie_secure,
            "OAuth2 authentication configured"
        );

        Ok(Some(Self {
            enabled: true,
            client_id,
            client_secret,
            authorize_url,
            token_url,
            userinfo_url,
            logout_url,
            redirect_uri,
            scope,
            cookie_secure,
        }))
    }

    fn test_config() -> Result<Self, AuthConfigError> {
        Ok(Self {
            enabled: true,
            client_id: "test-client".to_string(),
            client_secret: "test-secret".to_string(),
            authorize_url: parse_static_url(
                "ARGUS_OAUTH_AUTHORIZE_URL",
                "https://auth.example.test/saaslogin1/oauth2/authorize",
            )?,
            token_url: parse_static_url(
                "ARGUS_OAUTH_TOKEN_URL",
                "https://auth.example.test/saaslogin1/oauth2/accesstoken",
            )?,
            userinfo_url: parse_static_url(
                "ARGUS_OAUTH_USERINFO_URL",
                "https://auth.example.test/saaslogin1/oauth2/userinfo",
            )?,
            logout_url: Some(parse_static_url(
                "ARGUS_OAUTH_LOGOUT_URL",
                "https://auth.example.test/saaslogin1/oauth2/logout",
            )?),
            redirect_uri: "http://127.0.0.1:3000/auth/callback".to_string(),
            scope: DEFAULT_SCOPE.to_string(),
            cookie_secure: false,
        })
    }
}

impl AuthState {
    #[must_use]
    pub fn disabled() -> Self {
        Self::new(None)
    }

    pub fn from_env() -> Result<Self, AuthConfigError> {
        Ok(Self::new(AuthConfig::from_env()?))
    }

    #[doc(hidden)]
    pub fn enabled_for_test() -> Result<Self, AuthConfigError> {
        Ok(Self::new(Some(AuthConfig::test_config()?)))
    }

    fn new(config: Option<AuthConfig>) -> Self {
        Self {
            inner: std::sync::Arc::new(AuthStateInner {
                config,
                http: reqwest::Client::new(),
                login_states: RwLock::new(HashMap::new()),
                sessions: RwLock::new(HashMap::new()),
            }),
        }
    }

    #[must_use]
    pub fn is_enabled(&self) -> bool {
        self.inner
            .config
            .as_ref()
            .map(|config| config.enabled)
            .unwrap_or(false)
    }

    #[doc(hidden)]
    #[must_use]
    pub async fn insert_session_for_test(&self, external_id: &str) -> String {
        let session_id = new_secret();
        let session = AuthSession {
            user: AuthenticatedUser::new(external_id.to_string(), None),
            access_token: None,
            refresh_token: None,
            expires_at: Some(Utc::now() + TimeDelta::seconds(SESSION_TTL_SECONDS)),
        };
        self.inner
            .sessions
            .write()
            .await
            .insert(session_id.clone(), session);
        format!("{SESSION_COOKIE_NAME}={session_id}")
    }

    #[must_use]
    pub fn session_cookie_name(&self) -> &'static str {
        SESSION_COOKIE_NAME
    }

    pub async fn login_location(&self, next: Option<&str>) -> Result<(String, String), ApiError> {
        let config = self
            .inner
            .config
            .as_ref()
            .ok_or_else(|| ApiError::bad_request("OAuth2 login is not enabled"))?;
        let state = new_secret();
        let next = sanitize_next(next);
        self.inner.login_states.write().await.insert(
            state.clone(),
            OAuthLoginState {
                next,
                created_at: Utc::now(),
            },
        );

        let mut location = config.authorize_url.clone();
        location
            .query_pairs_mut()
            .append_pair("client_id", &config.client_id)
            .append_pair("response_type", "code")
            .append_pair("redirect_uri", &config.redirect_uri)
            .append_pair("scope", &config.scope)
            .append_pair("state", &state);
        Ok((location.into(), state_cookie(&state, config.cookie_secure)))
    }

    pub async fn complete_authorization(
        &self,
        code: &str,
        state: &str,
        headers: &HeaderMap,
    ) -> Result<(String, String), ApiError> {
        let config = self
            .inner
            .config
            .as_ref()
            .ok_or_else(|| ApiError::bad_request("OAuth2 login is not enabled"))?;
        let cookie_state = cookie_value(headers, LOGIN_STATE_COOKIE_NAME)
            .ok_or_else(|| ApiError::unauthorized("missing OAuth2 state cookie"))?;
        if cookie_state != state {
            return Err(ApiError::unauthorized("OAuth2 state mismatch"));
        }
        let login_state = self
            .inner
            .login_states
            .write()
            .await
            .remove(state)
            .ok_or_else(|| ApiError::unauthorized("OAuth2 state is invalid or expired"))?;
        if Utc::now() - login_state.created_at > TimeDelta::minutes(30) {
            return Err(ApiError::unauthorized("OAuth2 state is expired"));
        }

        let token = self.exchange_code(config, code).await?;
        let user_info = self.fetch_user_info(config, &token.access_token).await?;
        let session_id = new_secret();
        let session = AuthSession {
            user: AuthenticatedUser::new(user_info.uuid, None),
            access_token: Some(token.access_token),
            refresh_token: token.refresh_token,
            expires_at: token.expires_in.map(expires_at),
        };
        self.inner
            .sessions
            .write()
            .await
            .insert(session_id.clone(), session);

        Ok((
            session_cookie(&session_id, config.cookie_secure),
            login_state.next,
        ))
    }

    pub async fn authenticate_request(
        &self,
        headers: &HeaderMap,
    ) -> Result<Option<AuthenticatedUser>, ApiError> {
        if !self.is_enabled() {
            return Ok(None);
        }
        let Some(session_id) = cookie_value(headers, SESSION_COOKIE_NAME) else {
            return Ok(None);
        };
        self.authenticate_session(&session_id).await
    }

    pub async fn clear_session(&self, headers: &HeaderMap) -> String {
        if let Some(session_id) = cookie_value(headers, SESSION_COOKIE_NAME) {
            self.inner.sessions.write().await.remove(&session_id);
        }
        clear_cookie(SESSION_COOKIE_NAME)
    }

    pub fn logout_location(&self, redirect: Option<&str>) -> String {
        let Some(config) = self.inner.config.as_ref() else {
            return sanitize_next(redirect);
        };
        let redirect = sanitize_next(redirect);
        let Some(logout_url) = config.logout_url.as_ref() else {
            return redirect;
        };
        let mut location = logout_url.clone();
        location
            .query_pairs_mut()
            .append_pair("clientId", &config.client_id)
            .append_pair("redirect", &redirect);
        location.into()
    }

    pub fn clear_login_state_cookie(&self) -> String {
        clear_cookie(LOGIN_STATE_COOKIE_NAME)
    }

    async fn authenticate_session(
        &self,
        session_id: &str,
    ) -> Result<Option<AuthenticatedUser>, ApiError> {
        let session = self.inner.sessions.read().await.get(session_id).cloned();
        let Some(session) = session else {
            return Ok(None);
        };
        if !session_needs_refresh(&session) {
            return Ok(Some(session.user));
        }
        let Some(refresh_token) = session.refresh_token.clone() else {
            self.inner.sessions.write().await.remove(session_id);
            return Ok(None);
        };
        let Some(config) = self.inner.config.as_ref() else {
            return Ok(None);
        };
        let refreshed = self.refresh_token(config, &refresh_token).await?;
        let mut sessions = self.inner.sessions.write().await;
        if let Some(current) = sessions.get_mut(session_id) {
            current.access_token = Some(refreshed.access_token);
            current.refresh_token = refreshed.refresh_token.or(current.refresh_token.clone());
            current.expires_at = refreshed.expires_in.map(expires_at);
            return Ok(Some(current.user.clone()));
        }
        Ok(None)
    }

    async fn exchange_code(
        &self,
        config: &AuthConfig,
        code: &str,
    ) -> Result<TokenResponse, ApiError> {
        self.inner
            .http
            .post(config.token_url.clone())
            .json(&TokenRequest {
                client_id: &config.client_id,
                client_secret: &config.client_secret,
                redirect_uri: &config.redirect_uri,
                grant_type: "authorization_code",
                code,
            })
            .send()
            .await
            .map_err(|error| ApiError::internal(format!("OAuth2 token request failed: {error}")))?
            .error_for_status()
            .map_err(|error| {
                ApiError::unauthorized(format!("OAuth2 token request was rejected: {error}"))
            })?
            .json::<TokenResponse>()
            .await
            .map_err(|error| {
                ApiError::internal(format!("OAuth2 token response was invalid: {error}"))
            })
    }

    async fn fetch_user_info(
        &self,
        config: &AuthConfig,
        access_token: &str,
    ) -> Result<UserInfoResponse, ApiError> {
        self.inner
            .http
            .post(config.userinfo_url.clone())
            .json(&UserInfoRequest {
                client_id: &config.client_id,
                access_token,
                scope: &config.scope,
            })
            .send()
            .await
            .map_err(|error| {
                ApiError::internal(format!("OAuth2 userinfo request failed: {error}"))
            })?
            .error_for_status()
            .map_err(|error| {
                ApiError::unauthorized(format!("OAuth2 userinfo request was rejected: {error}"))
            })?
            .json::<UserInfoResponse>()
            .await
            .map_err(|error| {
                ApiError::internal(format!("OAuth2 userinfo response was invalid: {error}"))
            })
    }

    async fn refresh_token(
        &self,
        config: &AuthConfig,
        refresh_token: &str,
    ) -> Result<TokenResponse, ApiError> {
        self.inner
            .http
            .post(config.token_url.clone())
            .json(&RefreshTokenRequest {
                client_id: &config.client_id,
                grant_type: "refresh_token",
                refresh_token,
            })
            .send()
            .await
            .map_err(|error| ApiError::internal(format!("OAuth2 refresh request failed: {error}")))?
            .error_for_status()
            .map_err(|error| {
                ApiError::unauthorized(format!("OAuth2 refresh request was rejected: {error}"))
            })?
            .json::<TokenResponse>()
            .await
            .map_err(|error| {
                ApiError::internal(format!("OAuth2 refresh response was invalid: {error}"))
            })
    }
}

pub async fn require_api_auth(
    State(state): State<AppState>,
    mut request: Request<Body>,
    next: Next,
) -> Result<Response<Body>, ApiError> {
    let path = request.uri().path();
    if !state.auth().is_enabled() || !path.starts_with("/api/v1/") || path == "/api/v1/health" {
        return Ok(next.run(request).await);
    }

    let user = state
        .auth()
        .authenticate_request(request.headers())
        .await?
        .ok_or_else(|| ApiError::unauthorized("authentication required"))?;
    request.extensions_mut().insert(user);
    Ok(next.run(request).await)
}

fn session_needs_refresh(session: &AuthSession) -> bool {
    session
        .expires_at
        .map(|expires_at| expires_at <= Utc::now() + TimeDelta::seconds(REFRESH_SKEW_SECONDS))
        .unwrap_or(false)
}

fn expires_at(expires_in_seconds: i64) -> DateTime<Utc> {
    Utc::now() + TimeDelta::seconds(expires_in_seconds)
}

fn new_secret() -> String {
    Uuid::new_v4().to_string()
}

fn sanitize_next(next: Option<&str>) -> String {
    match next.map(str::trim).filter(|value| !value.is_empty()) {
        Some(value) if value.starts_with('/') && !value.starts_with("//") => value.to_string(),
        _ => "/".to_string(),
    }
}

fn env_bool_value(value: Option<&str>) -> bool {
    value.map(parse_bool).unwrap_or(false)
}

fn env_value_present(value: Option<&str>) -> bool {
    value.map(str::trim).is_some_and(|value| !value.is_empty())
}

fn env_value_for_log(value: Option<&str>) -> &str {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("<not set>")
}

fn parse_bool(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn required_value(name: &'static str, value: Option<&str>) -> Result<String, AuthConfigError> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
        .ok_or(AuthConfigError::MissingEnv(name))
}

fn endpoint_url(
    name: &'static str,
    explicit: Option<&str>,
    base_url: Option<&Url>,
    path: &str,
) -> Result<Url, AuthConfigError> {
    if let Some(explicit) = explicit.map(str::trim).filter(|value| !value.is_empty()) {
        return parse_url(name, explicit);
    }
    let base_url = base_url.ok_or(AuthConfigError::MissingEnv(name))?;
    base_url
        .join(path)
        .map_err(|source| AuthConfigError::InvalidUrl { name, source })
}

fn optional_endpoint_url(
    name: &'static str,
    explicit: Option<&str>,
    base_url: Option<&Url>,
    path: &str,
) -> Result<Option<Url>, AuthConfigError> {
    if let Some(explicit) = explicit.map(str::trim).filter(|value| !value.is_empty()) {
        return parse_url(name, explicit).map(Some);
    }
    base_url
        .map(|base_url| {
            base_url
                .join(path)
                .map_err(|source| AuthConfigError::InvalidUrl { name, source })
        })
        .transpose()
}

fn parse_static_url(name: &'static str, value: &str) -> Result<Url, AuthConfigError> {
    parse_url(name, value)
}

fn parse_url(name: &'static str, value: &str) -> Result<Url, AuthConfigError> {
    Url::parse(value).map_err(|source| AuthConfigError::InvalidUrl { name, source })
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get(header::COOKIE)
        .and_then(|value| value.to_str().ok())
        .and_then(|cookie| {
            cookie.split(';').find_map(|part| {
                let mut pieces = part.trim().splitn(2, '=');
                let cookie_name = pieces.next()?.trim();
                let cookie_value = pieces.next()?.trim();
                (cookie_name == name && !cookie_value.is_empty()).then(|| cookie_value.to_string())
            })
        })
}

fn session_cookie(session_id: &str, secure: bool) -> String {
    format_cookie(
        SESSION_COOKIE_NAME,
        session_id,
        secure,
        Some(Duration::from_secs(SESSION_TTL_SECONDS as u64)),
    )
}

fn state_cookie(state: &str, secure: bool) -> String {
    format_cookie(
        LOGIN_STATE_COOKIE_NAME,
        state,
        secure,
        Some(Duration::from_secs(30 * 60)),
    )
}

fn clear_cookie(name: &str) -> String {
    format!("{name}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0")
}

fn format_cookie(name: &str, value: &str, secure: bool, max_age: Option<Duration>) -> String {
    let mut cookie = format!("{name}={value}; Path=/; HttpOnly; SameSite=Lax");
    if secure {
        cookie.push_str("; Secure");
    }
    if let Some(max_age) = max_age {
        cookie.push_str(&format!("; Max-Age={}", max_age.as_secs()));
    }
    cookie
}

pub(crate) fn header_value(value: &str) -> Result<HeaderValue, ApiError> {
    HeaderValue::from_str(value).map_err(|_| ApiError::internal("response header was invalid"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_derives_oauth_endpoints_from_base_url() {
        let config = AuthConfig::from_env_values(AuthEnvValues {
            enabled: Some("true"),
            client_id: Some("client"),
            client_secret: Some("secret"),
            base_url: Some("https://auth.example.test"),
            authorize_url: None,
            token_url: None,
            userinfo_url: None,
            logout_url: None,
            redirect_uri: Some("http://127.0.0.1:3010/auth/callback"),
            scope: None,
            cookie_secure: None,
        })
        .expect("base url config should parse")
        .expect("enabled config should be present");

        assert_eq!(
            config.authorize_url.as_str(),
            "https://auth.example.test/saaslogin1/oauth2/authorize"
        );
        assert_eq!(
            config.token_url.as_str(),
            "https://auth.example.test/saaslogin1/oauth2/accesstoken"
        );
        assert_eq!(
            config.userinfo_url.as_str(),
            "https://auth.example.test/saaslogin1/oauth2/userinfo"
        );
        assert_eq!(
            config.logout_url.as_ref().map(Url::as_str),
            Some("https://auth.example.test/saaslogin1/oauth2/logout")
        );
        assert_eq!(config.scope, DEFAULT_SCOPE);
    }

    #[test]
    fn env_value_present_trims_empty_values() {
        assert!(!env_value_present(None));
        assert!(!env_value_present(Some("")));
        assert!(!env_value_present(Some("  ")));
        assert!(env_value_present(Some("client")));
    }

    #[test]
    fn parse_bool_accepts_trimmed_case_insensitive_values() {
        assert!(parse_bool("true"));
        assert!(parse_bool(" True "));
        assert!(parse_bool("YES"));
        assert!(parse_bool(" on "));
        assert!(parse_bool("1"));
        assert!(!parse_bool("false"));
    }

    #[test]
    fn sanitize_next_rejects_external_urls() {
        assert_eq!(sanitize_next(Some("/chat")), "/chat");
        assert_eq!(sanitize_next(Some("https://example.test")), "/");
        assert_eq!(sanitize_next(Some("//example.test")), "/");
    }
}
