use std::sync::Arc;

use argus_repository::migrate;
use argus_wing::ArgusWing;
use axum::Router;
use axum::body::{Body, to_bytes};
use axum::http::{Method, Request, Response};
use serde::Serialize;
use serde::de::DeserializeOwned;
use tower::util::ServiceExt;

#[allow(dead_code)]
pub struct TestContext {
    pub app: Router,
    pub wing: Arc<ArgusWing>,
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
        let wing = ArgusWing::with_pool(pool);
        let app = argus_server::router(argus_server::app_state::AppState::new(Arc::clone(&wing)));
        Self { app, wing }
    }

    pub async fn get(&self, path: &str) -> Response<Body> {
        self.request(Method::GET, path, Option::<&()>::None).await
    }

    pub async fn post_json<T>(&self, path: &str, payload: &T) -> Response<Body>
    where
        T: Serialize,
    {
        self.request(Method::POST, path, Some(payload)).await
    }

    pub async fn patch_json<T>(&self, path: &str, payload: &T) -> Response<Body>
    where
        T: Serialize,
    {
        self.request(Method::PATCH, path, Some(payload)).await
    }

    pub async fn put_json<T>(&self, path: &str, payload: &T) -> Response<Body>
    where
        T: Serialize,
    {
        self.request(Method::PUT, path, Some(payload)).await
    }

    async fn request<T>(&self, method: Method, path: &str, payload: Option<&T>) -> Response<Body>
    where
        T: Serialize,
    {
        let mut request = Request::builder().method(method).uri(path);
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

pub async fn json_body<T>(response: Response<Body>) -> T
where
    T: DeserializeOwned,
{
    let bytes = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("response body should be readable");
    serde_json::from_slice(&bytes).expect("response body should deserialize from json")
}
