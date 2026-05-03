mod support;

use axum::http::StatusCode;

#[tokio::test]
async fn oauth_enabled_protects_all_api_routes_except_health() {
    let auth = argus_server::auth::AuthState::enabled_for_test()
        .expect("test auth config should be valid");
    let ctx = support::TestContext::new_with_auth(auth).await;

    let health = ctx.get("/api/v1/health").await;
    assert_eq!(health.status(), StatusCode::OK);

    let bootstrap = ctx.get("/api/v1/bootstrap").await;
    assert_eq!(bootstrap.status(), StatusCode::UNAUTHORIZED);

    let chat = ctx.get("/api/v1/chat/sessions").await;
    assert_eq!(chat.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn oauth_session_cookie_authenticates_api_requests() {
    let auth = argus_server::auth::AuthState::enabled_for_test()
        .expect("test auth config should be valid");
    let cookie = auth.insert_session_for_test("oauth-user-1").await;
    let ctx = support::TestContext::new_with_auth(auth).await;

    let me = ctx.get_with_cookie("/api/v1/auth/me", &cookie).await;
    assert_eq!(me.status(), StatusCode::OK);

    let chat = ctx.get_with_cookie("/api/v1/chat/sessions", &cookie).await;
    assert_eq!(chat.status(), StatusCode::OK);
}

#[tokio::test]
async fn oauth_login_redirects_to_authorization_endpoint() {
    let auth = argus_server::auth::AuthState::enabled_for_test()
        .expect("test auth config should be valid");
    let ctx = support::TestContext::new_with_auth(auth).await;

    let response = ctx.get("/auth/login?next=/chat").await;
    assert_eq!(response.status(), StatusCode::SEE_OTHER);
    let location = response
        .headers()
        .get("location")
        .expect("login should set location")
        .to_str()
        .expect("location should be valid");
    assert!(location.starts_with("https://auth.example.test/saaslogin1/oauth2/authorize?"));
    assert!(location.contains("client_id=test-client"));
    assert!(location.contains("response_type=code"));
    assert!(location.contains("scope=base.profile"));
}
