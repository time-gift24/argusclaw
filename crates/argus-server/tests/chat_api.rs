//! Integration tests for the user chat HTTP API.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use argus_protocol::{AgentId, AgentRecord, SessionId, ThreadEvent, ThreadId, UserRecord};
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
use chrono::Utc;
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

        for user in users.iter_mut() {
            if user.external_subject == identity.external_subject {
                user.account = identity.account.clone();
                user.display_name = identity.display_name.clone();
                return Ok(user.clone());
            }
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

struct FakeChatService {
    sessions: Mutex<HashMap<i64, Vec<SessionSummary>>>,
    threads: Mutex<HashMap<SessionId, Vec<ThreadSummary>>>,
}

impl FakeChatService {
    fn new() -> Self {
        Self {
            sessions: Mutex::new(HashMap::new()),
            threads: Mutex::new(HashMap::new()),
        }
    }
}

#[async_trait]
impl UserChatApi for FakeChatService {
    async fn list_enabled_agents(&self) -> Vec<AgentRecord> {
        vec![AgentRecord {
            id: AgentId::new(1),
            display_name: "Enabled".to_string(),
            is_enabled: true,
            ..AgentRecord::default()
        }]
    }

    async fn create_session(
        &self,
        user: &UserPrincipal,
        name: &str,
    ) -> Result<SessionId, UserChatError> {
        let session_id = SessionId::new();
        let session = SessionSummary {
            id: session_id,
            name: name.to_string(),
            thread_count: 1,
            updated_at: Utc::now(),
        };
        self.sessions
            .lock()
            .unwrap()
            .entry(user.user_id)
            .or_default()
            .push(session.clone());
        self.threads.lock().unwrap().insert(
            session_id,
            vec![ThreadSummary {
                id: ThreadId::new(),
                title: Some(format!("{name} thread")),
                turn_count: 0,
                token_count: 0,
                updated_at: Utc::now(),
            }],
        );
        Ok(session_id)
    }

    async fn list_sessions(
        &self,
        user: &UserPrincipal,
    ) -> Result<Vec<SessionSummary>, UserChatError> {
        Ok(self
            .sessions
            .lock()
            .unwrap()
            .get(&user.user_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn list_threads(
        &self,
        user: &UserPrincipal,
        session_id: SessionId,
    ) -> Result<Vec<ThreadSummary>, UserChatError> {
        let sessions = self.sessions.lock().unwrap();
        let owned = sessions
            .get(&user.user_id)
            .map(|items| items.iter().any(|session| session.id == session_id))
            .unwrap_or(false);
        drop(sessions);

        if !owned {
            return Err(UserChatError::NotFound);
        }

        Ok(self
            .threads
            .lock()
            .unwrap()
            .get(&session_id)
            .cloned()
            .unwrap_or_default())
    }

    async fn send_message(
        &self,
        user: &UserPrincipal,
        session_id: SessionId,
        _thread_id: ThreadId,
        _message: String,
    ) -> Result<(), UserChatError> {
        let sessions = self.sessions.lock().unwrap();
        let owned = sessions
            .get(&user.user_id)
            .map(|items| items.iter().any(|session| session.id == session_id))
            .unwrap_or(false);
        if owned {
            Ok(())
        } else {
            Err(UserChatError::NotFound)
        }
    }

    async fn subscribe(
        &self,
        user: &UserPrincipal,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Result<broadcast::Receiver<ThreadEvent>, UserChatError> {
        let threads = self.list_threads(user, session_id).await?;
        if !threads.iter().any(|thread| thread.id == thread_id) {
            return Err(UserChatError::NotFound);
        }

        let (sender, receiver) = broadcast::channel(4);
        let _ = sender.send(ThreadEvent::Idle {
            thread_id: thread_id.to_string(),
        });
        Ok(receiver)
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
        chat_services: Arc::new(FakeChatService::new()) as Arc<dyn UserChatApi>,
    };

    argus_server::build_router(state)
}

async fn login_as(app: &Router, account: &str, display_name: &str) -> String {
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
    let state_cookie = login
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .find_map(|value| {
            let cookie = value.to_str().ok()?;
            cookie
                .split(';')
                .find(|part| part.trim().starts_with("argus_oauth_state="))
                .map(|part| part.trim().to_string())
        })
        .unwrap();
    let location = login
        .headers()
        .get(header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap();
    let state = location.split("state=").nth(1).unwrap();

    let form_body = format!(
        "state={state}&account={account}&display_name={}",
        display_name.replace(' ', "+")
    );
    let authorize = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/dev-oauth/authorize")
                .header(header::COOKIE, &state_cookie)
                .header(header::CONTENT_TYPE, "application/x-www-form-urlencoded")
                .body(Body::from(form_body))
                .unwrap(),
        )
        .await
        .unwrap();
    let callback_location = authorize
        .headers()
        .get(header::LOCATION)
        .unwrap()
        .to_str()
        .unwrap();

    let callback = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(callback_location)
                .header(header::COOKIE, &state_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();

    callback
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
        .unwrap()
}

#[tokio::test]
async fn unauthenticated_requests_are_rejected() {
    let response = app()
        .oneshot(
            Request::builder()
                .uri("/api/agents")
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn authenticated_user_can_list_agents_and_sessions() {
    let app = app();
    let cookie = login_as(&app, "alice@example.com", "Alice").await;

    let create = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sessions")
                .header(header::COOKIE, &cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"name":"alice"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(create.status(), StatusCode::OK);

    let agents = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/agents")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(agents.status(), StatusCode::OK);

    let sessions = app
        .clone()
        .oneshot(
            Request::builder()
                .uri("/api/sessions")
                .header(header::COOKIE, &cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(sessions.status(), StatusCode::OK);
    let body = sessions.into_body().collect().await.unwrap().to_bytes();
    let list: Vec<SessionSummary> = serde_json::from_slice(&body).unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "alice");
}

#[tokio::test]
async fn cross_user_thread_access_returns_not_found() {
    let app = app();
    let alice_cookie = login_as(&app, "alice@example.com", "Alice").await;
    let bob_cookie = login_as(&app, "bob@example.com", "Bob").await;

    let create = app
        .clone()
        .oneshot(
            Request::builder()
                .method("POST")
                .uri("/api/sessions")
                .header(header::COOKIE, &alice_cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(r#"{"name":"alice"}"#))
                .unwrap(),
        )
        .await
        .unwrap();
    let create_body = create.into_body().collect().await.unwrap().to_bytes();
    let created: serde_json::Value = serde_json::from_slice(&create_body).unwrap();
    let session_id = created["id"].as_str().unwrap().to_string();

    let threads = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/sessions/{session_id}/threads"))
                .header(header::COOKIE, &alice_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    let thread_body = threads.into_body().collect().await.unwrap().to_bytes();
    let summaries: Vec<ThreadSummary> = serde_json::from_slice(&thread_body).unwrap();
    let thread_id = summaries[0].id.to_string();

    let response = app
        .clone()
        .oneshot(
            Request::builder()
                .uri(format!("/api/sessions/{session_id}/threads"))
                .header(header::COOKIE, &bob_cookie)
                .body(Body::empty())
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let send = app
        .oneshot(
            Request::builder()
                .method("POST")
                .uri(format!("/api/threads/{thread_id}/messages"))
                .header(header::COOKIE, &bob_cookie)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(format!(
                    r#"{{"session_id":"{session_id}","content":"hello"}}"#
                )))
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(send.status(), StatusCode::NOT_FOUND);
}
