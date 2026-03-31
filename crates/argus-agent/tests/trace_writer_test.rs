//! Tests for TraceWriter.

use argus_agent::TurnLogEvent;
use argus_agent::trace::{TraceConfig, TraceWriter, read_jsonl_events};
use argus_protocol::TokenUsage;
use argus_protocol::llm::ChatMessage;

#[tokio::test]
async fn test_trace_writer_success() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config = TraceConfig::new(true, temp_dir.path().to_path_buf());

    // base_dir = {trace_dir}/{session_id}/{thread_id}
    let thread_dir = temp_dir.path().join("session-1").join("thread-1");
    let mut writer = TraceWriter::new(&thread_dir, 1, &config).await.unwrap();

    let req_event = TurnLogEvent::LlmRequest {
        messages: vec![],
        tools: vec![],
    };
    writer.write_event(&req_event).await.unwrap();

    let resp_event = TurnLogEvent::LlmResponse {
        content: "Hello".to_string(),
        reasoning_content: None,
        tool_calls: vec![],
        finish_reason: "stop".to_string(),
        metadata: None,
    };
    writer.write_event(&resp_event).await.unwrap();

    let token_usage = TokenUsage {
        input_tokens: 10,
        output_tokens: 5,
        total_tokens: 15,
    };
    writer.finish_success(&token_usage).await.unwrap();

    let trace_path = thread_dir.join("turns").join("1.jsonl");
    assert!(trace_path.exists());

    let events = read_jsonl_events(&trace_path).await.unwrap();
    assert_eq!(events.len(), 3);
    assert!(matches!(events[0], TurnLogEvent::LlmRequest { .. }));
    assert!(matches!(events[1], TurnLogEvent::LlmResponse { .. }));
    assert!(matches!(events[2], TurnLogEvent::TurnEnd { .. }));
}

#[tokio::test]
async fn test_trace_writer_failure() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config = TraceConfig::new(true, temp_dir.path().to_path_buf());

    let thread_dir = temp_dir.path().join("session-1").join("thread-1");
    let mut writer = TraceWriter::new(&thread_dir, 1, &config).await.unwrap();

    let req_event = TurnLogEvent::LlmRequest {
        messages: vec![],
        tools: vec![],
    };
    writer.write_event(&req_event).await.unwrap();

    let resp_event = TurnLogEvent::LlmResponse {
        content: String::new(),
        reasoning_content: None,
        tool_calls: vec![],
        finish_reason: "error".to_string(),
        metadata: None,
    };
    writer.write_event(&resp_event).await.unwrap();

    writer.finish_failure("Test error message").await.unwrap();

    let trace_path = thread_dir.join("turns").join("1.jsonl");
    assert!(trace_path.exists());

    let events = read_jsonl_events(&trace_path).await.unwrap();
    assert_eq!(events.len(), 3);
    assert!(matches!(events[0], TurnLogEvent::LlmRequest { .. }));
    assert!(matches!(events[1], TurnLogEvent::LlmResponse { .. }));
    assert!(matches!(events[2], TurnLogEvent::TurnError { .. }));
}

#[tokio::test]
async fn test_trace_writer_with_tool_execution() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config = TraceConfig::new(true, temp_dir.path().to_path_buf());

    let thread_dir = temp_dir.path().join("session-1").join("thread-1");
    let mut writer = TraceWriter::new(&thread_dir, 1, &config).await.unwrap();

    let req_event = TurnLogEvent::LlmRequest {
        messages: vec![ChatMessage::user("test")],
        tools: vec![],
    };
    writer.write_event(&req_event).await.unwrap();

    let tool_event = TurnLogEvent::ToolResult {
        id: "call_1".to_string(),
        name: "echo".to_string(),
        result: "hello".to_string(),
        duration_ms: 42,
        error: None,
    };
    writer.write_event(&tool_event).await.unwrap();

    let resp_event = TurnLogEvent::LlmResponse {
        content: String::new(),
        reasoning_content: None,
        tool_calls: vec![],
        finish_reason: "stop".to_string(),
        metadata: None,
    };
    writer.write_event(&resp_event).await.unwrap();

    let token_usage = TokenUsage {
        input_tokens: 100,
        output_tokens: 50,
        total_tokens: 150,
    };
    writer.finish_success(&token_usage).await.unwrap();

    let trace_path = thread_dir.join("turns").join("1.jsonl");
    assert!(trace_path.exists());

    let content = tokio::fs::read_to_string(&trace_path).await.unwrap();
    assert!(content.contains("\"echo\""));
    assert!(content.contains("\"duration_ms\":42"));
}
