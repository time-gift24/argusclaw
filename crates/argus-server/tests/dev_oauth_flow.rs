//! Integration tests for the dev OAuth2 flow.

use std::sync::{Arc, Mutex};

use argus_protocol::{SessionId, ThreadEvent, ThreadId, UserRecord};
use argus_repository::{DbError, UserRepository};
use argus_server::auth::dev_oauth::DevOAuth2Provider;
use argus_server::auth::provider::OAuth2AuthProvider;
use argus_server::auth::session::AuthSession;
use argus_server::config::ServerConfig;
use argus_server::state::AppState;
use argus_session::{SessionSummary, ThreadSummary, UserChatApi, UserChatError, UserPrincipal};
use async_trait::async_trait;
use axum::Router;
use axum::body::Body;
use axum::http::{Request, StatusCode, header};
use http_body_util::BodyExt;
use tokio::sync::broadcast;
use tower::ServiceExt;

struct InMemoryUserRepo {
    users: Mutex<Vec<UserRecord>>,
}

impl InMemoryUserRepo {
    fn new() -> Self {
        Self {
            users: Mutex::new(Vec::new()),
        }
    }
}

#[async_trait]
impl UserRepository for InMemoryUserRepo {
    async fn upsert_from_oauth2(
        &self,
        identity: &argus_protocol::OAuth2Identity,
    ) -> Result<UserRecord, DbError> {
        let mut users = self.users.lock().map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        if let Some(user) = users
            .iter_mut()
            .find(|user| user.external_subject == identity.external_subject)
        {
            user.account = identity.account.clone();
            user.display_name = identity.display_name.clone();
            return Ok(user.clone());
        }

        let user = UserRecord {
            id: users.len() as i64 + 1,
            external_subject: identity.external_subject.clone(),
            account: identity.account.clone(),
            display_name: identity.display_name.clone(),
        };
        users.push(user.clone());
        Ok(user)
    }

    async fn get_by_id(&self, id: i64) -> Result<Option<UserRecord>, DbError> {
        let users = self.users.lock().map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        Ok(users.iter().find(|user| user.id == id).cloned())
    }
}

struct DummyChatService;

#[async_trait]
impl UserChatApi for DummyChatService {
    async fn list_enabled_agents(&self) -> Vec<argus_protocol::AgentRecord> {
        Vec::new()
    }

    async fn create_session(
        &self,
        _user: &UserPrincipal,
        _name: &str,
    ) -> Result<SessionId, UserChatError> {
        Err(UserChatError::NotFound)
    }

    async fn list_sessions(
        &self,
        _user: &UserPrincipal,
    ) -> Result<Vec<SessionSummary>, UserChatError> {
        Ok(Vec::new())
    }

    async fn list_threads(
        &self,
        _user: &UserPrincipal,
        _session_id: SessionId,
    ) -> Result<Vec<ThreadSummary>, UserChatError> {
        Ok(Vec::new())
    }

    async fn send_message(
        &self,
        _user: &UserPrincipal,
        _session_id: SessionId,
        _thread_id: ThreadId,
        _message: String,
    ) -> Result<(), UserChatError> {
        Err(UserChatError::NotFound)
    }

    async fn subscribe(
        &self,
        _user: &UserPrincipal,
        _session_id: SessionId,
        _thread_id: ThreadId,
    ) -> Result<broadcast::Receiver<ThreadEvent>, UserChatError> {
        Err(UserChatError::NotFound)
    }
}

fn app() -> Router {
    let mut config = ServerConfig::default();
    config.secure_cookies = false;

    let state = AppState {
        config: Arc::new(config),
        auth_provider: Arc::new(DevOAuth2Provider::new()) as Arc<dyn OAuth2AuthProvider>,
        user_repo: Arc::new(InMemoryUserRepo::new()) as Arc<dyn UserRepository>,
        auth_session: Arc::new(AuthSession::new("test-secret")),
        chat_services: Arc::new(DummyChatService) as Arc<dyn UserChatApi>,
    };

    argus_server::build_router(state)
}

fn oauth_state_cookie(headers: &axum::http::HeaderMap) -> String {
    headers
        .get_all(header::SET_COOKIE)
        .iter()
        .find_map(|value| {
            let cookie = value.to_str().ok()?;
            cookie
                .split(';')
                .find(|part| part.trim().starts_with("argus_oauth_state="))
                .map(|part| part.trim().to_string())
        })
        .unwrap()
}

#[tokio::test]
async fn auth_login_redirects_to_dev_authorize_and_sets_state_cookie() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/auth/login")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT);
    let location = response
        .headers()
        .get(header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap();
    assert!(location.contains("/dev-oauth/authorize"));
    assert!(location.contains("state="));
    assert!(oauth_state_cookie(response.headers()).starts_with("argus_oauth_state="));
}

#[tokio::test]
async fn callback_rejects_missing_or_mismatched_state_cookie() {
    let app = app();
    let login = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/auth/login")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let location = login
        .headers()
        .get(header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap();
    let state = location.split("state=").nth(1).unwrap();

    let authorize = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/dev-oauth/authorize")
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!(
                    "state={state}&account=test@example.com&display_name=Test"
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    let callback_uri = authorize
        .headers()
        .get(header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap();

    let missing_cookie = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(callback_uri)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(missing_cookie.status(), StatusCode::UNAUTHORIZED);

    let bad_cookie = app
        .oneshot(
            Request::builder()
                .uri(callback_uri)
                .header(header::COOKIE, "argus_oauth_state=bad")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(bad_cookie.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn callback_upserts_user_and_sets_session_cookie() {
    let app = app();
    let login = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/auth/login")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let state_cookie = oauth_state_cookie(login.headers());
    let location = login
        .headers()
        .get(header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap();
    let state = location.split("state=").nth(1).unwrap();

    let authorize = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/dev-oauth/authorize")
                .header(header::COOKIE, &state_cookie)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!(
                    "state={state}&account=alice@example.com&display_name=Alice"
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    let callback_uri = authorize
        .headers()
        .get(header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap();

    let callback = app
        .oneshot(
            Request::builder()
                .uri(callback_uri)
                .header(header::COOKIE, &state_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(callback.status(), StatusCode::TEMPORARY_REDIRECT);

    let cookies: Vec<String> = callback
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|value| value.to_str().unwrap().to_string())
        .collect();
    assert!(
        cookies
            .iter()
            .any(|cookie| cookie.contains("argus_session="))
    );
    assert!(
        cookies
            .iter()
            .any(|cookie| cookie.contains("argus_oauth_state="))
    );
}

#[tokio::test]
async fn api_me_returns_authenticated_user() {
    let app = app();
    let login = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/auth/login")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let state_cookie = oauth_state_cookie(login.headers());
    let location = login
        .headers()
        .get(header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap();
    let state = location.split("state=").nth(1).unwrap();

    let authorize = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/dev-oauth/authorize")
                .header(header::COOKIE, &state_cookie)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(format!(
                    "state={state}&account=bob@example.com&display_name=Bob"
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    let callback_uri = authorize
        .headers()
        .get(header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap();

    let callback = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(callback_uri)
                .header(header::COOKIE, &state_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let session_cookie = callback
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .find_map(|value| {
            let cookie = value.to_str().ok()?;
            cookie
                .split(';')
                .find(|part| part.trim().starts_with("argus_session="))
                .map(|part| part.trim().to_string())
        })
        .unwrap();

    let response = app
        .oneshot(
            Request::builder()
                .uri("/api/me")
                .header(header::COOKIE, session_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(response.status(), StatusCode::OK);
    let body = response.into_body().collect().await.unwrap().to_bytes();
    let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
    assert_eq!(json["account"], "bob@example.com");
    assert_eq!(json["display_name"], "Bob");
}
