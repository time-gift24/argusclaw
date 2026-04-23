mod support;

use axum::http::StatusCode;
use serde_json::Value;

#[tokio::test]
async fn tools_route_lists_registered_builtin_tools() {
    let ctx = support::TestContext::new().await;

    let response = ctx.get("/api/v1/tools").await;
    assert_eq!(response.status(), StatusCode::OK);

    let tools: Vec<Value> = support::json_body(response).await;
    let names: Vec<&str> = tools
        .iter()
        .filter_map(|tool| tool.get("name").and_then(Value::as_str))
        .collect();

    for expected in [
        "shell",
        "read",
        "grep",
        "glob",
        "http",
        "write_file",
        "list_dir",
        "apply_patch",
        "sleep",
        "chrome",
        "scheduler",
    ] {
        assert!(
            names.contains(&expected),
            "expected tool registry to include {expected}, got {names:?}"
        );
    }

    let shell = tools
        .iter()
        .find(|tool| tool.get("name").and_then(Value::as_str) == Some("shell"))
        .expect("shell tool should be listed");
    assert_eq!(
        shell.get("risk_level").and_then(Value::as_str),
        Some("critical")
    );
    assert!(
        shell.get("parameters").is_some_and(Value::is_object),
        "tool definitions should include parameter schemas"
    );
}
