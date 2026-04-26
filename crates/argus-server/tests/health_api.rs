use axum::body::{Body, to_bytes};
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

#[tokio::test]
async fn web_dist_serves_index_for_spa_routes() {
    let temp_dir = tempfile::tempdir().expect("web dist temp dir should be created");
    std::fs::write(
        temp_dir.path().join("index.html"),
        "<main>ArgusWing Web</main>",
    )
    .expect("index should be written");

    let app = argus_server::router_for_test_with_web_dist(temp_dir.path().to_path_buf()).await;
    let response = app
        .oneshot(
            Request::get("/agent-runs")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("response should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should be readable");
    assert_eq!(body.as_ref(), b"<main>ArgusWing Web</main>");
}

#[tokio::test]
async fn api_routes_take_precedence_over_web_dist_fallback() {
    let temp_dir = tempfile::tempdir().expect("web dist temp dir should be created");
    std::fs::write(
        temp_dir.path().join("index.html"),
        "<main>ArgusWing Web</main>",
    )
    .expect("index should be written");

    let app = argus_server::router_for_test_with_web_dist(temp_dir.path().to_path_buf()).await;
    let response = app
        .oneshot(
            Request::get("/api/v1/health")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("response should succeed");

    assert_eq!(response.status(), StatusCode::OK);
    let body = to_bytes(response.into_body(), usize::MAX)
        .await
        .expect("body should be readable");
    let body_text = String::from_utf8(body.to_vec()).expect("health body should be utf8");
    assert!(body_text.contains("\"status\":\"ok\""));
}

#[tokio::test]
async fn unknown_api_routes_do_not_fall_back_to_web_dist() {
    let temp_dir = tempfile::tempdir().expect("web dist temp dir should be created");
    std::fs::write(
        temp_dir.path().join("index.html"),
        "<main>ArgusWing Web</main>",
    )
    .expect("index should be written");

    let app = argus_server::router_for_test_with_web_dist(temp_dir.path().to_path_buf()).await;
    let response = app
        .oneshot(
            Request::get("/api/v1/does-not-exist")
                .body(Body::empty())
                .expect("request should build"),
        )
        .await
        .expect("response should succeed");

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
}
