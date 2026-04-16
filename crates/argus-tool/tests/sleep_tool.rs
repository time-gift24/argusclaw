use std::sync::Arc;

use argus_protocol::ids::ThreadId;
use argus_protocol::{RiskLevel, ToolError, ToolExecutionContext};
use argus_tool::{NamedTool, SleepTool};
use serde_json::json;
use tokio::sync::broadcast;

fn make_ctx() -> Arc<ToolExecutionContext> {
    let (tx, _) = broadcast::channel(16);
    Arc::new(ToolExecutionContext {
        thread_id: ThreadId::new(),
        agent_id: None,
        pipe_tx: tx,
    })
}

#[test]
fn sleep_tool_name_risk_and_schema_are_declared() {
    let tool = SleepTool::new();
    let definition = tool.definition();

    assert_eq!(tool.name(), "sleep");
    assert_eq!(tool.risk_level(), RiskLevel::Low);
    assert_eq!(definition.name, "sleep");
    assert_eq!(definition.parameters["required"], json!(["duration_ms"]),);
    assert_eq!(
        definition.parameters["properties"]["duration_ms"]["minimum"],
        json!(1),
    );
    assert_eq!(
        definition.parameters["properties"]["duration_ms"]["maximum"],
        json!(120000),
    );
    assert_eq!(definition.parameters["additionalProperties"], json!(false));
}

#[tokio::test(start_paused = true)]
async fn sleep_tool_waits_and_returns_elapsed_duration() {
    let tool = SleepTool::new();

    let result = tool
        .execute(json!({ "duration_ms": 1000 }), make_ctx())
        .await
        .expect("sleep should succeed");

    assert_eq!(result, json!({ "slept_ms": 1000 }));
}

#[tokio::test]
async fn sleep_tool_rejects_missing_duration() {
    let tool = SleepTool::new();

    let err = tool
        .execute(json!({}), make_ctx())
        .await
        .expect_err("missing duration should fail");

    assert!(matches!(err, ToolError::ExecutionFailed { .. }));
    assert!(err.to_string().contains("duration_ms"));
}

#[tokio::test]
async fn sleep_tool_rejects_zero_duration() {
    let tool = SleepTool::new();

    let err = tool
        .execute(json!({ "duration_ms": 0 }), make_ctx())
        .await
        .expect_err("zero duration should fail");

    assert!(matches!(err, ToolError::ExecutionFailed { .. }));
    assert!(err.to_string().contains("between 1 and 120000"));
}

#[tokio::test]
async fn sleep_tool_rejects_duration_above_cap() {
    let tool = SleepTool::new();

    let err = tool
        .execute(json!({ "duration_ms": 120001 }), make_ctx())
        .await
        .expect_err("over-cap duration should fail");

    assert!(matches!(err, ToolError::ExecutionFailed { .. }));
    assert!(err.to_string().contains("between 1 and 120000"));
}

#[tokio::test]
async fn sleep_tool_rejects_unknown_fields() {
    let tool = SleepTool::new();

    let err = tool
        .execute(json!({ "duration_ms": 1, "extra": true }), make_ctx())
        .await
        .expect_err("unknown fields should fail");

    assert!(matches!(err, ToolError::ExecutionFailed { .. }));
    assert!(err.to_string().contains("unknown field"));
}
