//! Integration tests for trace generation.

use std::fs;
use std::sync::{Arc, Mutex};

use tokio::sync::broadcast;

use argus_protocol::AgentRecord;
use argus_protocol::llm::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LlmError, LlmEventStream,
    LlmProvider, LlmStreamEvent, ToolCall, ToolCallDelta, ToolCompletionRequest,
    ToolCompletionResponse, ToolDefinition,
};
use argus_protocol::tool::{NamedTool, ToolError};
use argus_turn::trace::TraceConfig;
use argus_turn::{TurnBuilder, TurnConfig};
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
            content: self.response.clone(),
            reasoning_content: None,
            input_tokens: 10,
            output_tokens: 5,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
            reasoning_tokens: 0,
        })
    }

    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        Ok(ToolCompletionResponse {
            content: Some(self.response.clone()),
            reasoning_content: None,
            tool_calls: vec![],
            input_tokens: 10,
            output_tokens: 5,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
            reasoning_tokens: 0,
        })
    }

    async fn stream_complete(
        &self,
        _request: CompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        Err(LlmError::RequestFailed {
            provider: "mock".to_string(),
            reason: "not implemented".to_string(),
        })
    }

    async fn stream_complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
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

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value, ToolError> {
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
        Err(LlmError::RequestFailed {
            provider: "mock".to_string(),
            reason: "not implemented".to_string(),
        })
    }

    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        let (content, tool_calls) = self.next_response();
        let finish_reason = if tool_calls.is_empty() {
            FinishReason::Stop
        } else {
            FinishReason::ToolUse
        };

        Ok(ToolCompletionResponse {
            content: Some(content),
            reasoning_content: None,
            tool_calls,
            input_tokens: 10,
            output_tokens: 5,
            finish_reason,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
            reasoning_tokens: 0,
        })
    }

    async fn stream_complete(
        &self,
        _request: CompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        Err(LlmError::RequestFailed {
            provider: "mock".to_string(),
            reason: "not implemented".to_string(),
        })
    }

    async fn stream_complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
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
    let trace_path = temp_dir.path().join("test-thread").join("1.json");
    assert!(
        trace_path.exists(),
        "Trace file should exist at {:?}",
        trace_path
    );

    // Parse and verify JSON structure
    let content = fs::read_to_string(&trace_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();

    // Verify required top-level keys (TraceFile structure)
    assert!(json.get("version").is_some());
    assert!(json.get("thread_id").is_some());
    assert!(json.get("turn_number").is_some());
    assert!(json.get("start_time").is_some());
    assert!(json.get("iterations").is_some());

    // Verify final_output is present (success case)
    assert!(
        json.get("final_output").is_some(),
        "Should have final_output on success"
    );

    // Verify we have at least one iteration
    let iterations = json["iterations"].as_array().unwrap();
    assert!(!iterations.is_empty(), "Should have at least one iteration");
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
    let trace_path = temp_dir.path().join("test-thread-tools").join("1.json");
    assert!(
        trace_path.exists(),
        "Trace file should exist at {:?}",
        trace_path
    );

    // Parse and verify JSON structure
    let content = fs::read_to_string(&trace_path).unwrap();
    let json: serde_json::Value = serde_json::from_str(&content).unwrap();

    // Verify we have iterations
    let iterations = json["iterations"].as_array().unwrap();
    assert!(!iterations.is_empty(), "Should have at least one iteration");

    // First iteration should have tool_calls in response
    let first_iter = &iterations[0];
    assert!(
        first_iter["llm_response"]["tool_calls"].is_array(),
        "First iteration should have tool_calls"
    );
    let tool_calls = first_iter["llm_response"]["tool_calls"].as_array().unwrap();
    assert!(!tool_calls.is_empty(), "Should have at least one tool call");

    // If we have tools in trace, verify structure
    if let Some(tools) = first_iter["tools"].as_array()
        && !tools.is_empty()
    {
        assert!(tools[0].get("name").is_some());
        assert!(tools[0].get("result").is_some());
    }
}

#[tokio::test]
async fn test_turn_trace_disabled_by_default() {
    // Default config should have tracing disabled
    let config = TraceConfig::default();
    assert!(!config.enabled, "Trace should be disabled by default");
}
