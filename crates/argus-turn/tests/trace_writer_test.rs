//! Tests for TraceWriter.

use argus_protocol::TokenUsage;
use argus_turn::trace::{
    IterationRecord, LlmRequest, LlmResponse, ToolExecution, TraceConfig, TraceWriter,
};
use std::fs;

#[tokio::test]
async fn test_trace_writer_success() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config = TraceConfig::new(true, temp_dir.path().to_path_buf());

    let mut writer = TraceWriter::new("thread-1", 1, &config).unwrap();

    let iteration = IterationRecord {
        iteration: 0,
        llm_request: LlmRequest {
            messages: vec![],
            tools: vec![],
        },
        llm_response: LlmResponse {
            content: Some("Hello".to_string()),
            reasoning_content: None,
            tool_calls: vec![],
            finish_reason: "stop".to_string(),
            input_tokens: 10,
            output_tokens: 5,
        },
        tools: vec![],
    };

    writer.write_iteration(iteration).unwrap();

    let token_usage = TokenUsage {
        input_tokens: 10,
        output_tokens: 5,
        total_tokens: 15,
        reasoning_tokens: 0,
    };
    writer.finish_success(&token_usage).unwrap();

    let trace_path = temp_dir.path().join("thread-1").join("1.json");
    assert!(trace_path.exists());

    let content = fs::read_to_string(&trace_path).unwrap();
    assert!(content.contains("\"iteration\":0"));
    assert!(content.contains("\"content\":\"Hello\""));
    assert!(content.contains("\"final_output\""));
}

#[tokio::test]
async fn test_trace_writer_failure() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config = TraceConfig::new(true, temp_dir.path().to_path_buf());

    let mut writer = TraceWriter::new("thread-1", 1, &config).unwrap();

    let iteration = IterationRecord {
        iteration: 0,
        llm_request: LlmRequest {
            messages: vec![],
            tools: vec![],
        },
        llm_response: LlmResponse {
            content: None,
            reasoning_content: None,
            tool_calls: vec![],
            finish_reason: "error".to_string(),
            input_tokens: 0,
            output_tokens: 0,
        },
        tools: vec![],
    };

    writer.write_iteration(iteration).unwrap();
    writer.finish_failure("Test error message").unwrap();

    let trace_path = temp_dir.path().join("thread-1").join("1.json");
    assert!(trace_path.exists());

    let content = fs::read_to_string(&trace_path).unwrap();
    assert!(content.contains("\"final_output\":null"));
}

#[tokio::test]
async fn test_trace_writer_with_tool_execution() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config = TraceConfig::new(true, temp_dir.path().to_path_buf());

    let mut writer = TraceWriter::new("thread-1", 1, &config).unwrap();

    let iteration = IterationRecord {
        iteration: 0,
        llm_request: LlmRequest {
            messages: vec![],
            tools: vec![],
        },
        llm_response: LlmResponse {
            content: None,
            reasoning_content: None,
            tool_calls: vec![],
            finish_reason: "stop".to_string(),
            input_tokens: 100,
            output_tokens: 50,
        },
        tools: vec![ToolExecution {
            id: "call_1".to_string(),
            name: "echo".to_string(),
            arguments: serde_json::json!({"message": "hello"}),
            result: "hello".to_string(),
            duration_ms: 42,
            error: None,
        }],
    };

    writer.write_iteration(iteration).unwrap();

    let token_usage = TokenUsage {
        input_tokens: 100,
        output_tokens: 50,
        total_tokens: 150,
        reasoning_tokens: 0,
    };
    writer.finish_success(&token_usage).unwrap();

    let trace_path = temp_dir.path().join("thread-1").join("1.json");
    let content = fs::read_to_string(&trace_path).unwrap();

    // Verify tool execution is recorded
    assert!(content.contains("\"name\":\"echo\""));
    assert!(content.contains("\"duration_ms\":42"));
}
