use std::sync::Arc;

use argus_protocol::McpDiscoveredToolRecord;
use argus_repository::traits::McpRepository;
use argus_repository::{ArgusSqlite, migrate};
use argus_server::server_core::ServerCore;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{HeaderMap, Method, Request, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tower::util::ServiceExt;

pub const DEFAULT_TEST_USER_ID: &str = "test-chat-user";
#[allow(dead_code)]
pub const ALT_TEST_USER_ID: &str = "alt-chat-user";
pub const POSTGRES_TEST_URL_ENV: &str = "ARGUS_TEST_POSTGRES_URL";
const DEFAULT_REQUEST_USER_ID: &str = DEFAULT_TEST_USER_ID;

#[allow(dead_code)]
pub struct TestContext {
    pub app: Router,
    pub core: Arc<ServerCore>,
    pool: Option<sqlx::SqlitePool>,
}

#[allow(dead_code)]
impl TestContext {
    pub async fn new() -> Self {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("sqlite pool should connect for tests");
        migrate(&pool)
            .await
            .expect("test migrations should succeed");
        let core = ServerCore::with_pool(pool.clone())
            .await
            .expect("server core should initialize for tests");
        let app = argus_server::router(argus_server::app_state::AppState::new(Arc::clone(&core)));
        Self {
            app,
            core,
            pool: Some(pool),
        }
    }

    pub async fn postgres_if_configured() -> Option<Self> {
        let database_url = match std::env::var(POSTGRES_TEST_URL_ENV) {
            Ok(database_url) => database_url,
            Err(_) => {
                eprintln!("skipping PostgreSQL server test: {POSTGRES_TEST_URL_ENV} is not set");
                return None;
            }
        };
        let core = ServerCore::init(Some(&database_url))
            .await
            .expect("server core should initialize against postgres test database");
        mark_postgres_test_user_admin(&database_url).await;
        let app = argus_server::router(argus_server::app_state::AppState::new(Arc::clone(&core)));
        Some(Self {
            app,
            core,
            pool: None,
        })
    }

    pub async fn get(&self, path: &str) -> Response<Body> {
        self.request(Method::GET, path, Option::<&()>::None).await
    }

    pub async fn get_without_chat_user(&self, path: &str) -> Response<Body> {
        self.request_with_chat_user(Method::GET, path, Option::<&()>::None, false)
            .await
    }

    pub async fn get_without_default_user_header(&self, path: &str) -> Response<Body> {
        self.get_without_chat_user(path).await
    }

    pub async fn get_as(&self, path: &str, user_id: &str) -> Response<Body> {
        self.request_as(Method::GET, path, Option::<&()>::None, user_id)
            .await
    }

    pub async fn get_with_cookie(&self, path: &str, cookie: &str) -> Response<Body> {
        let mut headers = HeaderMap::new();
        headers.insert(
            "cookie",
            cookie.parse().expect("test cookie header should be valid"),
        );
        self.request_with_headers(Method::GET, path, Option::<&()>::None, headers)
            .await
    }

    pub async fn new_with_auth(auth: argus_server::auth::AuthState) -> Self {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("sqlite pool should connect for tests");
        migrate(&pool)
            .await
            .expect("test migrations should succeed");
        let core = ServerCore::with_pool(pool.clone())
            .await
            .expect("server core should initialize for tests");
        let app = argus_server::router(argus_server::app_state::AppState::with_auth(
            Arc::clone(&core),
            auth,
        ));
        Self {
            app,
            core,
            pool: Some(pool),
        }
    }

    pub async fn post_json<T>(&self, path: &str, payload: &T) -> Response<Body>
    where
        T: Serialize,
    {
        self.request(Method::POST, path, Some(payload)).await
    }

    pub async fn post_json_as<T>(&self, path: &str, payload: &T, user_id: &str) -> Response<Body>
    where
        T: Serialize,
    {
        self.request_as(Method::POST, path, Some(payload), user_id)
            .await
    }

    pub async fn post_empty(&self, path: &str) -> Response<Body> {
        self.request(Method::POST, path, Option::<&()>::None).await
    }

    pub async fn patch_json<T>(&self, path: &str, payload: &T) -> Response<Body>
    where
        T: Serialize,
    {
        self.request(Method::PATCH, path, Some(payload)).await
    }

    pub async fn patch_json_as<T>(&self, path: &str, payload: &T, user_id: &str) -> Response<Body>
    where
        T: Serialize,
    {
        self.request_as(Method::PATCH, path, Some(payload), user_id)
            .await
    }

    pub async fn put_json<T>(&self, path: &str, payload: &T) -> Response<Body>
    where
        T: Serialize,
    {
        self.request(Method::PUT, path, Some(payload)).await
    }

    pub async fn delete(&self, path: &str) -> Response<Body> {
        self.request(Method::DELETE, path, Option::<&()>::None)
            .await
    }

    pub async fn delete_as(&self, path: &str, user_id: &str) -> Response<Body> {
        self.request_as(Method::DELETE, path, Option::<&()>::None, user_id)
            .await
    }

    async fn request_as<T>(
        &self,
        method: Method,
        path: &str,
        payload: Option<&T>,
        user_id: &str,
    ) -> Response<Body>
    where
        T: Serialize,
    {
        let mut headers = HeaderMap::new();
        headers.insert(
            "x-argus-user-id",
            user_id
                .parse()
                .expect("test user id header should be valid"),
        );
        self.request_with_headers(method, path, payload, headers)
            .await
    }

    pub async fn request_without_trusted_user<T>(
        &self,
        method: Method,
        path: &str,
        payload: Option<&T>,
    ) -> Response<Body>
    where
        T: Serialize,
    {
        self.request_with_headers(method, path, payload, HeaderMap::new())
            .await
    }

    pub async fn seed_mcp_tools(&self, server_id: i64, tools: Vec<McpDiscoveredToolRecord>) {
        let pool = self
            .pool
            .clone()
            .expect("MCP seed helper is only available for SQLite test contexts");
        let sqlite = ArgusSqlite::new(pool);
        McpRepository::replace_mcp_server_tools(&sqlite, server_id, &tools)
            .await
            .expect("mcp tools should seed");
    }

    async fn request<T>(&self, method: Method, path: &str, payload: Option<&T>) -> Response<Body>
    where
        T: Serialize,
    {
        self.request_with_chat_user(method, path, payload, true)
            .await
    }

    async fn request_with_chat_user<T>(
        &self,
        method: Method,
        path: &str,
        payload: Option<&T>,
        include_chat_user: bool,
    ) -> Response<Body>
    where
        T: Serialize,
    {
        let mut headers = HeaderMap::new();
        if include_chat_user && should_attach_default_user(path) {
            headers.insert(
                "x-argus-user-id",
                DEFAULT_REQUEST_USER_ID
                    .parse()
                    .expect("test user id header should be valid"),
            );
        }
        self.request_with_headers(method, path, payload, headers)
            .await
    }

    async fn request_with_headers<T>(
        &self,
        method: Method,
        path: &str,
        payload: Option<&T>,
        headers: HeaderMap,
    ) -> Response<Body>
    where
        T: Serialize,
    {
        let mut request = Request::builder().method(method).uri(path);
        for (name, value) in headers {
            if let Some(name) = name {
                request = request.header(name, value);
            }
        }
        let body = match payload {
            Some(payload) => {
                request = request.header("content-type", "application/json");
                Body::from(
                    serde_json::to_vec(payload).expect("request payload should serialize to json"),
                )
            }
            None => Body::empty(),
        };

        self.app
            .clone()
            .oneshot(request.body(body).expect("request should build"))
            .await
            .expect("response should succeed")
    }
}

async fn mark_postgres_test_user_admin(database_url: &str) {
    let pool = sqlx::PgPool::connect(database_url)
        .await
        .expect("postgres admin seed pool should connect");
    sqlx::query(
        "INSERT INTO users (id, external_id, display_name, is_admin, created_at, updated_at)
         VALUES ($1, $2, $3, TRUE, CURRENT_TIMESTAMP::TEXT, CURRENT_TIMESTAMP::TEXT)
         ON CONFLICT(external_id) DO UPDATE SET is_admin = TRUE, updated_at = CURRENT_TIMESTAMP::TEXT",
    )
    .bind(uuid::Uuid::new_v4())
    .bind(DEFAULT_TEST_USER_ID)
    .bind("Test Admin")
    .execute(&pool)
    .await
    .expect("postgres test user should be marked as admin");
}

fn should_attach_default_user(path: &str) -> bool {
    path.starts_with("/api/v1/")
        && !path.starts_with("/api/v1/health")
        && !path.starts_with("/api/v1/auth/")
}

#[allow(dead_code)]
pub async fn json_body<T>(response: Response<Body>) -> T
where
    T: DeserializeOwned,
{
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should be readable");
    serde_json::from_slice(&bytes).expect("response body should deserialize from json")
}
