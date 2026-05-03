mod support;

use axum::http::StatusCode;

const ORDINARY_USER_ID: &str = "ordinary-user";

#[tokio::test]
async fn non_admin_user_cannot_access_management_routes_but_can_access_chat_routes() {
    let ctx = support::TestContext::new().await;

    let management = ctx.get_as("/api/v1/runtime", ORDINARY_USER_ID).await;
    assert_eq!(management.status(), StatusCode::FORBIDDEN);
    let body: serde_json::Value = support::json_body(management).await;
    assert_eq!(body["error"]["code"], "forbidden");

    let chat = ctx.get_as("/api/v1/chat/sessions", ORDINARY_USER_ID).await;
    assert_eq!(chat.status(), StatusCode::OK);

    let chat_options = ctx.get_as("/api/v1/chat/options", ORDINARY_USER_ID).await;
    assert_eq!(chat_options.status(), StatusCode::OK);
}

#[tokio::test]
async fn bootstrap_returns_current_user_admin_flag() {
    let ctx = support::TestContext::new().await;

    let response = ctx.get_as("/api/v1/bootstrap", ORDINARY_USER_ID).await;

    assert_eq!(response.status(), StatusCode::OK);
    let body: serde_json::Value = support::json_body(response).await;
    assert_eq!(body["current_user"]["external_id"], ORDINARY_USER_ID);
    assert_eq!(body["current_user"]["is_admin"], false);
}
