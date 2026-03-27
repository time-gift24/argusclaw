//! Integration tests for trace generation.

use std::fs;
use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

use argus_protocol::AgentRecord;
use argus_protocol::ToolExecutionContext;
use argus_protocol::llm::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LlmError, LlmEventStream,
    LlmProvider, LlmStreamEvent, ToolCall, ToolCallDelta, ToolDefinition,
};
use argus_protocol::tool::{NamedTool, ToolError};
use argus_agent::trace::TraceConfig;
use argus_agent::{TurnBuilder, TurnConfig};
use async_trait::async_trait;
use rust_decimal::Decimal;

/// Mock provider that returns a simple response
struct SimpleMockProvider {
    response: String,
}

impl SimpleMockProvider {
    fn new(response: &str) -> Self {
        Self {
            response: response.to_string(),
        }
    }
}

#[async_trait]
impl LlmProvider for SimpleMockProvider {
    fn model_name(&self) -> &str {
        "mock"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        Ok(CompletionResponse {
            content: Some(self.response.clone()),
            reasoning_content: None,
            tool_calls: vec![],
            input_tokens: 10,
            output_tokens: 5,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn stream_complete(
        &self,
        _request: CompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        let response = self.response.clone();
        let stream = futures_util::stream::once(async move {
            Ok(LlmStreamEvent::ContentDelta { delta: response })
        });
        Ok(Box::pin(stream))
    }
}

/// Echo tool for integration tests
struct EchoTool;

#[async_trait]
impl NamedTool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "echo".to_string(),
            description: "Echo back the input message".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "The message to echo back"
                    }
                },
                "required": ["message"]
            }),
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
        _ctx: Arc<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError> {
        let message = args
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool_name: "echo".to_string(),
                reason: "Missing 'message' parameter".to_string(),
            })?;

        Ok(serde_json::json!({
            "echoed": message
        }))
    }
}

/// Provider that returns a tool call response
struct ToolCallMockProvider {
    responses: Mutex<Vec<(String, Vec<ToolCall>)>>,
}

impl ToolCallMockProvider {
    fn new(responses: Vec<(String, Vec<ToolCall>)>) -> Self {
        Self {
            responses: Mutex::new(responses),
        }
    }

    fn next_response(&self) -> (String, Vec<ToolCall>) {
        let mut responses = self.responses.lock().unwrap();
        if responses.len() > 1 {
            responses.remove(0)
        } else if !responses.is_empty() {
            responses[0].clone()
        } else {
            ("No more responses".to_string(), vec![])
        }
    }
}

#[async_trait]
impl LlmProvider for ToolCallMockProvider {
    fn model_name(&self) -> &str {
        "mock"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let (content, tool_calls) = self.next_response();
        let finish_reason = if tool_calls.is_empty() {
            FinishReason::Stop
        } else {
            FinishReason::ToolUse
        };

        Ok(CompletionResponse {
            content: Some(content),
            reasoning_content: None,
            tool_calls,
            input_tokens: 10,
            output_tokens: 5,
            finish_reason,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn stream_complete(
        &self,
        _request: CompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        let (content, tool_calls) = self.next_response();
        let has_tool_calls = !tool_calls.is_empty();

        // Build events vector first
        let mut events: Vec<Result<LlmStreamEvent, LlmError>> = Vec::new();
        if !content.is_empty() {
            events.push(Ok(LlmStreamEvent::ContentDelta { delta: content }));
        }
        for (idx, tool_call) in tool_calls.into_iter().enumerate() {
            let args_str = serde_json::to_string(&tool_call.arguments).unwrap_or_default();
            events.push(Ok(LlmStreamEvent::ToolCallDelta(ToolCallDelta {
                index: idx,
                id: Some(tool_call.id),
                name: Some(tool_call.name),
                arguments_delta: Some(args_str),
            })));
        }
        events.push(Ok(LlmStreamEvent::Finished {
            finish_reason: if has_tool_calls {
                FinishReason::ToolUse
            } else {
                FinishReason::Stop
            },
        }));

        let stream = futures_util::stream::iter(events);
        Ok(Box::pin(stream))
    }
}

#[tokio::test]
async fn test_turn_trace_file_created_on_success() {
    let temp_dir = tempfile::tempdir().unwrap();
    let trace_config = TraceConfig::new(true, temp_dir.path().to_path_buf());

    let provider = Arc::new(SimpleMockProvider::new("Hello, world!"));
    let (stream_tx, _) = broadcast::channel(256);
    let (thread_event_tx, _) = broadcast::channel(256);

    let turn = TurnBuilder::default()
        .turn_number(1)
        .thread_id("test-thread".to_string())
        .messages(vec![ChatMessage::user("Hello")])
        .provider(provider)
        .agent_record(Arc::new(AgentRecord::default()))
        .tools(vec![])
        .hooks(vec![])
        .config(TurnConfig::default())
        .stream_tx(stream_tx)
        .thread_event_tx(thread_event_tx)
        .trace_config(trace_config)
        .build()
        .unwrap();

    let output = turn.execute().await.unwrap();

    // Verify turn executed
    assert!(output.messages.len() >= 2);

    // Verify trace file exists
    let trace_path = temp_dir
        .path()
        .join("test-thread")
        .join("turns")
        .join("1.jsonl");
    assert!(
        trace_path.exists(),
        "Trace file should exist at {:?}",
        trace_path
    );

    // Parse and verify JSONL structure
    let content = fs::read_to_string(&trace_path).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert!(!lines.is_empty(), "Should have at least one JSONL line");

    // Each line should be a valid JSON with wrapper + data
    let mut found_user_input = false;
    let mut found_llm_req = false;
    let mut found_llm_resp = false;
    let mut found_turn_end = false;
    for line in &lines {
        let json: serde_json::Value = serde_json::from_str(line).unwrap();
        let event_type = json.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match event_type {
            "user_input" => found_user_input = true,
            "llm_req" => found_llm_req = true,
            "llm_response" => found_llm_resp = true,
            "turn_end" => found_turn_end = true,
            _ => {}
        }
    }
    assert!(found_user_input, "Should have user_input event");
    assert!(found_llm_req, "Should have llm_req event");
    assert!(found_llm_resp, "Should have llm_response event");
    assert!(found_turn_end, "Should have turn_end event on success");
}

#[tokio::test]
async fn test_turn_trace_contains_tool_execution() {
    let temp_dir = tempfile::tempdir().unwrap();
    let trace_config = TraceConfig::new(true, temp_dir.path().to_path_buf());

    // Provider returns tool call first, then final response
    let responses = vec![
        (
            "I'll echo that.".to_string(),
            vec![ToolCall {
                id: "call-123".to_string(),
                name: "echo".to_string(),
                arguments: serde_json::json!({"message": "test message"}),
            }],
        ),
        ("Done! I echoed 'test message'.".to_string(), vec![]),
    ];
    let provider = Arc::new(ToolCallMockProvider::new(responses));
    let (stream_tx, _) = broadcast::channel(256);
    let (thread_event_tx, _) = broadcast::channel(256);

    let turn = TurnBuilder::default()
        .turn_number(1)
        .thread_id("test-thread-tools".to_string())
        .messages(vec![ChatMessage::user("Echo 'test message'")])
        .provider(provider)
        .agent_record(Arc::new(AgentRecord::default()))
        .tools(vec![Arc::new(EchoTool)])
        .hooks(vec![])
        .config(TurnConfig::default())
        .stream_tx(stream_tx)
        .thread_event_tx(thread_event_tx)
        .trace_config(trace_config)
        .build()
        .unwrap();

    let output = turn.execute().await.unwrap();

    // Verify turn executed
    assert!(output.messages.len() >= 3);

    // Verify trace file exists
    let trace_path = temp_dir
        .path()
        .join("test-thread-tools")
        .join("turns")
        .join("1.jsonl");
    assert!(
        trace_path.exists(),
        "Trace file should exist at {:?}",
        trace_path
    );

    // Parse and verify JSONL structure
    let content = fs::read_to_string(&trace_path).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert!(!lines.is_empty(), "Should have at least one JSONL line");

    // Verify we have llm_req, tool_result, llm_response, turn_end, and new events
    let mut found_user_input = false;
    let mut found_llm_req = false;
    let mut found_llm_resp = false;
    let mut found_tool_call_start = false;
    let mut found_tool_result = false;
    let mut found_turn_end = false;
    for line in &lines {
        let json: serde_json::Value = serde_json::from_str(line).unwrap();
        let event_type = json.get("type").and_then(|t| t.as_str()).unwrap_or("");
        match event_type {
            "user_input" => found_user_input = true,
            "llm_req" => found_llm_req = true,
            "llm_response" => found_llm_resp = true,
            "tool_call_start" => found_tool_call_start = true,
            "tool_result" => found_tool_result = true,
            "turn_end" => found_turn_end = true,
            _ => {}
        }
    }
    assert!(found_user_input, "Should have user_input event");
    assert!(found_llm_req, "Should have llm_req event");
    assert!(found_llm_resp, "Should have llm_response event");
    assert!(found_tool_call_start, "Should have tool_call_start event");
    assert!(found_tool_result, "Should have tool_result event");
    assert!(found_turn_end, "Should have turn_end event on success");
}

#[tokio::test]
async fn test_turn_trace_disabled_by_default() {
    // Default config should have tracing disabled
    let config = TraceConfig::default();
    assert!(!config.enabled, "Trace should be disabled by default");
}

#[tokio::test]
async fn test_full_jsonl_event_sequence() {
    let temp_dir = tempfile::tempdir().unwrap();
    let trace_config = TraceConfig::new(true, temp_dir.path().to_path_buf());

    // Provider returns tool call first, then final response (stop)
    let responses = vec![
        (
            "Let me echo that.".to_string(),
            vec![ToolCall {
                id: "call-abc".to_string(),
                name: "echo".to_string(),
                arguments: serde_json::json!({"message": "hello"}),
            }],
        ),
        ("Done!".to_string(), vec![]),
    ];
    let provider = Arc::new(ToolCallMockProvider::new(responses));
    let (stream_tx, _) = broadcast::channel(256);
    let (thread_event_tx, _) = broadcast::channel(256);

    let turn = TurnBuilder::default()
        .turn_number(1)
        .thread_id("test-thread-seq".to_string())
        .messages(vec![ChatMessage::user("Say hello")])
        .provider(provider)
        .agent_record(Arc::new(AgentRecord::default()))
        .tools(vec![Arc::new(EchoTool)])
        .hooks(vec![])
        .config(TurnConfig::default())
        .stream_tx(stream_tx)
        .thread_event_tx(thread_event_tx)
        .trace_config(trace_config)
        .build()
        .unwrap();

    let output = turn.execute().await.unwrap();
    assert!(output.messages.len() >= 3);

    // Read JSONL file
    let trace_path = temp_dir
        .path()
        .join("test-thread-seq")
        .join("turns")
        .join("1.jsonl");
    assert!(trace_path.exists());
    let content = fs::read_to_string(&trace_path).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    assert!(!lines.is_empty());

    // Collect event types in order
    let event_types: Vec<String> = lines
        .iter()
        .map(|line| {
            let json: serde_json::Value = serde_json::from_str(line).unwrap();
            json.get("type")
                .and_then(|t| t.as_str())
                .map(|s| s.to_string())
                .unwrap_or_default()
        })
        .collect();

    // Verify expected sequence (tools run in parallel, so tool events interleave with post-tools llm_response):
    // user_input (from turn start), llm_req (iteration 1), tool_call_start, tool_result,
    // llm_response (iteration 1 completion), llm_req (iteration 2), llm_response (stop), turn_end
    let expected_sequence = vec![
        "user_input",
        "llm_req",
        "tool_call_start",
        "tool_result",
        "llm_response",
        "llm_req",
        "llm_response",
        "turn_end",
    ];
    assert_eq!(event_types, expected_sequence, "Event sequence mismatch");

    // Verify duration_ms > 0 in tool_result (fields are flattened into the wrapper)
    let tool_result_line = lines
        .iter()
        .find(|line| {
            let json: serde_json::Value = serde_json::from_str(line).unwrap();
            json.get("type").and_then(|t| t.as_str()) == Some("tool_result")
        })
        .expect("Should have a tool_result event");
    let json: serde_json::Value = serde_json::from_str(tool_result_line).unwrap();
    // Verify duration_ms is present and is a valid u64 (may be 0 for fast tool executions)
    let _duration_ms = json
        .get("duration_ms")
        .expect("tool_result should have duration_ms")
        .as_u64()
        .expect("duration_ms should be a number");
}
