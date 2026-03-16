use std::path::Path;

const USER_SERVICE_SOURCE: &str = include_str!("../src/user/service.rs");

#[test]
fn user_service_uses_runtime_sqlx_queries_without_offline_metadata() {
    assert!(
        !USER_SERVICE_SOURCE.contains("sqlx::query!("),
        "user service should use runtime sqlx::query calls instead of sqlx::query! macros"
    );
    assert!(
        !USER_SERVICE_SOURCE.contains("sqlx::query_as!("),
        "user service should use runtime sqlx::query_as calls instead of sqlx::query_as! macros"
    );
    assert!(
        !Path::new(env!("CARGO_MANIFEST_DIR")).join(".sqlx").exists(),
        "claw crate should not require a .sqlx offline metadata directory"
    );
}
