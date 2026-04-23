mod support;

use axum::http::StatusCode;

use argus_server::routes::bootstrap::BootstrapResponse;

#[tokio::test]
async fn bootstrap_returns_instance_summary() {
    let ctx = support::TestContext::new().await;
    let response = ctx.get("/api/v1/bootstrap").await;

    assert_eq!(response.status(), StatusCode::OK);

    let body: BootstrapResponse = support::json_body(response).await;
    assert_eq!(body.instance_name, "ArgusWing");
    assert!(body.provider_count >= 1);
    assert!(body.template_count >= 1);
    assert!(body.default_provider_id > 0);
}
