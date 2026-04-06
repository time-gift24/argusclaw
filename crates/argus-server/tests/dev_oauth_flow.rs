//! Integration tests for the dev OAuth2 flow.
//!
//! Covers:
//! - GET /auth/login redirects to the dev authorize route
//! - Authorize form submission leads to callback with code and state
//! - Callback upserts the user and establishes a cookie session
//! - GET /api/me returns the authenticated user

use std::sync::Arc;

use argus_protocol::UserRecord;
use argus_repository::{UserRepository, DbError};
use argus_server::auth::dev_oauth::DevOAuth2Provider;
use argus_server::auth::provider::OAuth2AuthProvider;
use argus_server::auth::session::AuthSession;
use argus_server::config::ServerConfig;
use argus_server::state::AppState;

use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use axum::Router;
use http_body_util::BodyExt;
use tower::ServiceExt;

/// In-memory user repository for testing.
struct InMemoryUserRepo {
    users: std::sync::Mutex<Vec<UserRecord>>,
}

impl InMemoryUserRepo {
    fn new() -> Self {
        Self {
            users: std::sync::Mutex::new(Vec::new()),
        }
    }
}

#[async_trait::async_trait]
impl UserRepository for InMemoryUserRepo {
    async fn upsert_from_oauth2(
        &self,
        identity: &argus_protocol::OAuth2Identity,
    ) -> Result<UserRecord, DbError> {
        let mut users = self.users.lock().map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        // Check existing
        for user in users.iter_mut() {
            if user.external_subject == identity.external_subject {
                user.account = identity.account.clone();
                user.display_name = identity.display_name.clone();
                return Ok(user.clone());
            }
        }
        // Insert new
        let record = UserRecord {
            id: (users.len() as i64) + 1,
            external_subject: identity.external_subject.clone(),
            account: identity.account.clone(),
            display_name: identity.display_name.clone(),
        };
        users.push(record.clone());
        Ok(record)
    }

    async fn get_by_id(&self, id: i64) -> Result<Option<UserRecord>, DbError> {
        let users = self.users.lock().map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        Ok(users.iter().find(|u| u.id == id).cloned())
    }
}

fn test_app() -> Router {
    let config = ServerConfig::default();
    let auth_provider = Arc::new(DevOAuth2Provider::new()) as Arc<dyn OAuth2AuthProvider>;
    let user_repo = Arc::new(InMemoryUserRepo::new()) as Arc<dyn UserRepository>;
    let auth_session = Arc::new(AuthSession::new("test-secret-key-for-dev-only"));
    let state = AppState {
        config: Arc::new(config),
        auth_provider,
        user_repo,
        auth_session,
    };
    argus_server::auth::routes::router().with_state(state)
}

/// Helper to build a POST body as Bytes to avoid type inference issues.
fn form_body(s: &str) -> axum::body::Body {
    Body::from(s.to_string())
}

#[tokio::test]
async fn auth_login_redirects_to_dev_authorize() {
    let app = test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/auth/login")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request");

    assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
    let location = response
        .headers()
        .get(header::LOCATION)
        .expect("location header")
        .to_str()
        .expect("location as str");
    assert!(
        location.contains("/dev-oauth/authorize"),
        "expected redirect to /dev-oauth/authorize, got: {location}"
    );
    assert!(
        location.contains("state="),
        "expected state parameter in redirect, got: {location}"
    );
}

#[tokio::test]
async fn dev_authorize_form_returns_html() {
    let app = test_app();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/dev-oauth/authorize?state=test-state-123")
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request");

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.expect("collect body").to_bytes();
    let html = String::from_utf8(body.to_vec()).expect("body as utf8");
    assert!(html.contains("<form"), "expected HTML form, got: {html}");
    assert!(
        html.contains("test-state-123"),
        "expected state value in form, got: {html}"
    );
}

#[tokio::test]
async fn authorize_submission_redirects_to_callback_with_code_and_state() {
    let app = test_app();

    let response = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/dev-oauth/authorize")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(form_body("state=test-state-456&account=testuser@example.com&display_name=TestUser"))
                .expect("build request"),
        )
        .await
        .expect("send request");

    assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
    let location = response
        .headers()
        .get(header::LOCATION)
        .expect("location header")
        .to_str()
        .expect("location as str");
    assert!(
        location.contains("/auth/callback?"),
        "expected redirect to /auth/callback, got: {location}"
    );
    assert!(
        location.contains("code="),
        "expected code parameter, got: {location}"
    );
    assert!(
        location.contains("state=test-state-456"),
        "expected state parameter, got: {location}"
    );
}

#[tokio::test]
async fn callback_upserts_user_and_sets_session_cookie() {
    let app = test_app();

    // First, get a valid code by posting the authorize form
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/dev-oauth/authorize")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(form_body("state=mystate&account=alice@example.com&display_name=Alice"))
                .expect("build request"),
        )
        .await
        .expect("send request");

    let location = response
        .headers()
        .get(header::LOCATION)
        .expect("location header")
        .to_str()
        .expect("location as str");

    // Extract code and state from the callback URL
    let callback_url = location.trim_start_matches("/auth/callback?");
    let params: std::collections::HashMap<String, String> = callback_url
        .split('&')
        .filter_map(|pair| {
            let mut kv = pair.splitn(2, '=');
            let k = kv.next()?;
            let v = kv.next()?;
            Some((k.to_string(), v.to_string()))
        })
        .collect();
    let code = params.get("code").expect("code param");
    let state = params.get("state").expect("state param");

    // Now call the callback
    let callback_uri = format!("/auth/callback?code={code}&state={state}");
    let response = app
        .oneshot(
            Request::builder()
                .uri(&callback_uri)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request");

    assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);

    // Check session cookie
    let set_cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .expect("set-cookie header")
        .to_str()
        .expect("cookie as str");
    assert!(
        set_cookie.contains("argus_session"),
        "expected argus_session cookie, got: {set_cookie}"
    );
}

#[tokio::test]
async fn api_me_returns_authenticated_user() {
    let app = test_app();

    // Complete the full login flow to get a session cookie
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/dev-oauth/authorize")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(form_body("state=me-state&account=bob@example.com&display_name=Bob"))
                .expect("build request"),
        )
        .await
        .expect("send request");

    let location = response
        .headers()
        .get(header::LOCATION)
        .expect("location header")
        .to_str()
        .expect("location as str");

    let callback_url = location.trim_start_matches("/auth/callback?");
    let params: std::collections::HashMap<String, String> = callback_url
        .split('&')
        .filter_map(|pair| {
            let mut kv = pair.splitn(2, '=');
            let k = kv.next()?;
            let v = kv.next()?;
            Some((k.to_string(), v.to_string()))
        })
        .collect();
    let code = params.get("code").expect("code param");
    let state = params.get("state").expect("state param");

    let callback_uri = format!("/auth/callback?code={code}&state={state}");
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(&callback_uri)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request");

    let set_cookie = response
        .headers()
        .get(header::SET_COOKIE)
        .expect("set-cookie header")
        .to_str()
        .expect("cookie as str");

    // Extract the cookie value (argus_session=...)
    let cookie_value = set_cookie
        .split(';')
        .find(|s| s.trim().starts_with("argus_session="))
        .expect("find session cookie")
        .trim();

    // Use the cookie to access /api/me
    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/me")
                .header(header::COOKIE, cookie_value)
                .body(Body::empty())
                .expect("build request"),
        )
        .await
        .expect("send request");

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.expect("collect body").to_bytes();
    let json: serde_json::Value =
        serde_json::from_slice(&body).expect("parse json response");
    assert_eq!(json["account"], "bob@example.com");
    assert_eq!(json["display_name"], "Bob");
}
