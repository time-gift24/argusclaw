mod support;

use axum::http::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Debug, Deserialize, Serialize)]
struct AccountStatus {
    configured: bool,
    username: Option<String>,
}

#[tokio::test]
async fn account_routes_configure_and_update_credentials_without_returning_password() {
    let ctx = support::TestContext::new().await;

    let initial_response = ctx.get("/api/v1/account").await;
    assert_eq!(initial_response.status(), StatusCode::OK);
    let initial: AccountStatus = support::json_body(initial_response).await;
    assert!(!initial.configured);
    assert_eq!(initial.username, None);

    let configure_response = ctx
        .put_json(
            "/api/v1/account",
            &json!({
                "username": "alice",
                "password": "first-secret",
            }),
        )
        .await;
    assert_eq!(configure_response.status(), StatusCode::OK);
    let configured: AccountStatus = support::json_body(configure_response).await;
    assert!(configured.configured);
    assert_eq!(configured.username.as_deref(), Some("alice"));

    let update_response = ctx
        .put_json(
            "/api/v1/account",
            &json!({
                "username": "bob",
                "password": "second-secret",
            }),
        )
        .await;
    assert_eq!(update_response.status(), StatusCode::OK);
    let updated: AccountStatus = support::json_body(update_response).await;
    assert!(updated.configured);
    assert_eq!(updated.username.as_deref(), Some("bob"));

    let body = serde_json::to_value(updated).expect("status should serialize");
    assert!(body.get("password").is_none());
}

#[tokio::test]
async fn account_routes_reject_empty_credentials_as_bad_request() {
    let ctx = support::TestContext::new().await;

    let blank_username = ctx
        .put_json(
            "/api/v1/account",
            &json!({
                "username": " ",
                "password": "secret",
            }),
        )
        .await;
    assert_eq!(blank_username.status(), StatusCode::BAD_REQUEST);

    let blank_password = ctx
        .put_json(
            "/api/v1/account",
            &json!({
                "username": "alice",
                "password": "",
            }),
        )
        .await;
    assert_eq!(blank_password.status(), StatusCode::BAD_REQUEST);
}
