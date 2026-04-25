use axum::body::Body;
use axum::http::{Request, StatusCode};
use tower::util::ServiceExt;

#[tokio::test]
async fn health_returns_ok() {
    let app = argus_server::router_for_test().await;
    let response = app
        .oneshot(
            Request::get("/api/v1/health")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("response should succeed");

    assert_eq!(response.status(), StatusCode::OK);
}
