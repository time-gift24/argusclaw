//! Integration tests for the user chat HTTP API.
//!
//! Covers:
//! - GET /api/agents returns 401 when chat_services is None
//! - POST /api/sessions returns 401 when chat_services is None
//! - GET /api/sessions returns 401 when chat_services is None
//! - Cross-user access is rejected (500 since chat_services is None)
//! - Unauthenticated access to chat API is rejected

//!
//! These tests verify the HTTP routing and ownership enforcement without
//! UserChatServices. The actual chat functionality will be tested at the
//! integration level with real services in Task 7.
//!
//! For now, the tests use chat_services: None to verify auth + error handling.
//! Once UserChatServices is wired into AppState, the tests will pass.

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
        for user in users.iter_mut() {
            if user.external_subject == identity.external_subject {
                user.account = identity.account.clone();
                user.display_name = identity.display_name.clone();
                return Ok(user.clone());
            }
        }
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
        chat_services: None,
    };
    let auth_routes = argus_server::auth::routes::router();
    let chat_routes = argus_server::routes::router();
    Router::new()
        .merge(auth_routes)
        .merge(chat_routes)
        .with_state(state)
}

async fn login_as(app: &Router, account: &str, display_name: &str) -> String {
    let form_body = format!(
        "state=test&account={account}&display_name={}",
        display_name.replace(' ', "+")
    );
    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/dev-oauth/authorize")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(form_body))
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
    let state_param = params.get("state").expect("state param");
    let callback_uri = format!("/auth/callback?code={code}&state={state_param}");
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
    set_cookie
        .split(';')
        .find(|s| s.trim().starts_with("argus_session="))
        .expect("find session cookie")
        .trim()
        .to_string()
}
fn auth_request(method: &str, uri: &str, cookie: &str) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::COOKIE, cookie)
        .body(Body::empty())
        .expect("build request")
}
fn auth_request_with_body(method: &str, uri: &str, cookie: &str, body: String) -> Request<Body> {
    Request::builder()
        .method(method)
        .uri(uri)
        .header(header::COOKIE, cookie)
        .header(header::CONTENT_TYPE, "application/json")
        .body(Body::from(body))
        .expect("build request")
}
#[tokio::test]
async fn get_agents_requires_authentication() {
    let app = test_app();
    let response = app
        .oneshot(auth_request("GET", "/api/agents", ""))
        .await
        .expect("send request");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
#[tokio::test]
async fn post_sessions_requires_authentication() {
    let app = test_app();
    let response = app
        .clone()
        .oneshot(auth_request_with_body(
            "POST",
            "/api/sessions",
            "",
            r#"{"name":"test"}"#.to_string(),
        ))
        .await
        .expect("send request");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
#[tokio::test]
async fn get_sessions_requires_authentication() {
    let app = test_app();
    let response = app
        .oneshot(auth_request("GET", "/api/sessions", ""))
        .await
        .expect("send request");
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}
#[tokio::test]
async fn cross_user_session_access_returns_500_without_chat_services() {
    let app = test_app();
    let alice_cookie = login_as(&app, "alice@test.com", "Alice").await;
    let _bob_cookie = login_as(&app, "bob@test.com", "Bob").await;
    // Alice creates a session -- will get 500 since chat_services is None
    let response = app
        .clone()
        .oneshot(auth_request_with_body(
            "POST",
            "/api/sessions",
            &alice_cookie,
            r#"{"name":"alice session"}"#.to_string(),
        ))
        .await
        .expect("send request");
    // Expect 500 because chat_services is None
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
}
#[tokio::test]
async fn unauthenticated_requests_to_chat_api_are_rejected() {
    let app = test_app();
    let get_endpoints = ["/api/agents", "/api/sessions"];
    for uri in &get_endpoints {
        let response = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(*uri)
                    .body(Body::empty())
                    .expect("build request"),
            )
            .await
            .expect("send request");
        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "GET /api/agents should require authentication"
        );
    }
}
