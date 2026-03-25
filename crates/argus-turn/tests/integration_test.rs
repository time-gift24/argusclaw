//! Integration tests for argus-turn

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::broadcast;

use argus_llm::retry::{RetryConfig, RetryProvider};
use argus_protocol::AgentRecord;
use argus_protocol::events::ThreadEvent;
use argus_protocol::llm::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LlmError, LlmEventStream,
    LlmProvider, LlmStreamEvent, Role, ToolCall, ToolCompletionRequest, ToolCompletionResponse,
    ToolDefinition,
};
use argus_protocol::tool::{NamedTool, ToolError};
use argus_protocol::ToolExecutionContext;
use argus_turn::{TurnBuilder, TurnConfig};
use async_trait::async_trait;
use rust_decimal::Decimal;

/// Mock provider that can simulate tool calls with stateful responses
struct MockProvider {
    /// Responses to return in sequence
    responses: Mutex<Vec<MockResponse>>,
}

/// A single mock response
struct MockResponse {
    content: String,
    tool_calls: Vec<ToolCall>,
}

impl MockProvider {
    /// Create a simple response provider
    fn new(response: String) -> Self {
        Self {
            responses: Mutex::new(vec![MockResponse {
                content: response,
                tool_calls: Vec::new(),
            }]),
        }
    }

    /// Create a provider with multiple responses (for tool call scenarios)
    fn with_responses(responses: Vec<(String, Vec<ToolCall>)>) -> Self {
        Self {
            responses: Mutex::new(
                responses
                    .into_iter()
                    .map(|(content, tool_calls)| MockResponse {
                        content,
                        tool_calls,
                    })
                    .collect(),
            ),
        }
    }

    /// Get the next response
    fn next_response(&self) -> MockResponse {
        let mut responses = self.responses.lock().unwrap();
        if responses.len() > 1 {
            responses.remove(0)
        } else if !responses.is_empty() {
            responses[0].clone()
        } else {
            MockResponse {
                content: "No more responses".to_string(),
                tool_calls: Vec::new(),
            }
        }
    }
}

impl Clone for MockResponse {
    fn clone(&self) -> Self {
        Self {
            content: self.content.clone(),
            tool_calls: self.tool_calls.clone(),
        }
    }
}

#[async_trait]
impl LlmProvider for MockProvider {
    fn model_name(&self) -> &str {
        "mock"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let response = self.next_response();
        Ok(CompletionResponse {
            content: response.content,
            reasoning_content: None,
            input_tokens: 10,
            output_tokens: 5,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        let response = self.next_response();
        let finish_reason = if response.tool_calls.is_empty() {
            FinishReason::Stop
        } else {
            FinishReason::ToolUse
        };

        Ok(ToolCompletionResponse {
            content: Some(response.content),
            reasoning_content: None,
            tool_calls: response.tool_calls,
            input_tokens: 10,
            output_tokens: 5,
            finish_reason,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
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

    async fn execute(&self, args: serde_json::Value, _ctx: Arc<ToolExecutionContext>) -> Result<serde_json::Value, ToolError> {
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

/// Mock provider that simulates transient failures for testing retry behavior
struct FlakyProvider {
    stream_failures: Mutex<usize>,
    calls: std::sync::atomic::AtomicUsize,
}

impl FlakyProvider {
    fn new(stream_failures: usize) -> Self {
        Self {
            stream_failures: Mutex::new(stream_failures),
            calls: std::sync::atomic::AtomicUsize::new(0),
        }
    }

    #[allow(dead_code)]
    fn calls(&self) -> usize {
        self.calls.load(std::sync::atomic::Ordering::Relaxed)
    }

    fn should_fail(&self) -> bool {
        // Try to decrement the failure counter
        // Returns true if we should fail (counter was > 0)
        // Returns false if we should succeed (counter was 0)
        let mut failures = self.stream_failures.lock().unwrap();
        if *failures > 0 {
            *failures -= 1;
            true
        } else {
            false
        }
    }
}

#[async_trait]
impl LlmProvider for FlakyProvider {
    fn model_name(&self) -> &str {
        "flaky"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        Err(LlmError::RequestFailed {
            provider: "flaky".to_string(),
            reason: "streaming not supported".to_string(),
        })
    }

    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        self.calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        if self
            .stream_failures
            .lock()
            .unwrap()
            .checked_sub(1)
            .is_some()
        {
            return Err(LlmError::RateLimited {
                provider: "flaky".to_string(),
                retry_after: Some(Duration::ZERO),
            });
        }

        Ok(ToolCompletionResponse {
            content: Some("Success after retries!".to_string()),
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
        Err(LlmError::RequestFailed {
            provider: "flaky".to_string(),
            reason: "use complete_with_tools".to_string(),
        })
    }

    async fn stream_complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        self.calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        if self.should_fail() {
            return Err(LlmError::RateLimited {
                provider: "flaky".to_string(),
                retry_after: Some(Duration::ZERO),
            });
        }

        // Create a simple stream that just emits a finish event
        let stream = futures_util::stream::once(async move {
            Ok(argus_protocol::llm::LlmStreamEvent::Finished {
                finish_reason: FinishReason::Stop,
            })
        });

        Ok(Box::pin(stream))
    }
}

#[tokio::test]
async fn test_turn_integration_simple() {
    let provider = Arc::new(MockProvider::new("Hello, world!".to_string()));

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
        .build()
        .unwrap();

    let output = turn.execute().await.unwrap();

    // Should have user + assistant messages
    assert_eq!(output.messages.len(), 2);
    assert_eq!(output.messages[0].role, Role::User);
    assert_eq!(output.messages[1].role, Role::Assistant);
    assert_eq!(output.messages[1].content, "Hello, world!");
}

#[tokio::test]
async fn test_turn_integration_with_tool_call() {
    // Create a provider with two responses:
    // 1. First response: call the echo tool
    // 2. Second response: final answer after tool execution
    let responses = vec![
        (
            "I'll echo that for you.".to_string(),
            vec![ToolCall {
                id: "call-123".to_string(),
                name: "echo".to_string(),
                arguments: serde_json::json!({"message": "test message"}),
            }],
        ),
        ("Done! I echoed 'test message' for you.".to_string(), vec![]),
    ];

    let provider = Arc::new(MockProvider::with_responses(responses));

    let (stream_tx, _) = broadcast::channel(256);
    let (thread_event_tx, _) = broadcast::channel(256);

    let turn = TurnBuilder::default()
        .turn_number(1)
        .thread_id("test-thread".to_string())
        .messages(vec![ChatMessage::user("Echo 'test message'")])
        .provider(provider)
        .agent_record(Arc::new(AgentRecord::default()))
        .tools(vec![Arc::new(EchoTool)])
        .hooks(vec![])
        .config(TurnConfig::default())
        .stream_tx(stream_tx)
        .thread_event_tx(thread_event_tx)
        .build()
        .unwrap();

    let output = turn.execute().await.unwrap();

    // Should have messages: user, assistant (with tool call), tool result, assistant (final)
    assert!(output.messages.len() >= 3);
    // Should have tracked tokens
    assert!(output.token_usage.total_tokens > 0);
    // First assistant message should have tool calls
    let assistant_msgs: Vec<_> = output
        .messages
        .iter()
        .filter(|m| m.role == Role::Assistant)
        .collect();
    assert!(!assistant_msgs.is_empty());
    // At least one assistant message should have tool calls
    assert!(
        assistant_msgs
            .iter()
            .any(|m| m.tool_calls.is_some() && !m.tool_calls.as_ref().unwrap().is_empty())
    );
}

#[tokio::test]
async fn test_turn_builder_validation() {
    let provider = Arc::new(MockProvider::new("test".to_string()));

    let (stream_tx, _) = broadcast::channel(256);
    let (thread_event_tx, _) = broadcast::channel(256);

    // Should fail without required fields
    let result = TurnBuilder::default()
        .messages(vec![ChatMessage::user("Hello")])
        .build();

    assert!(result.is_err());

    // Should succeed with all required fields
    let result = TurnBuilder::default()
        .turn_number(1)
        .thread_id("test".to_string())
        .messages(vec![ChatMessage::user("Hello")])
        .provider(provider)
        .agent_record(Arc::new(AgentRecord::default()))
        .tools(vec![])
        .hooks(vec![])
        .config(TurnConfig::default())
        .stream_tx(stream_tx)
        .thread_event_tx(thread_event_tx)
        .build();

    assert!(result.is_ok());
}

#[tokio::test]
async fn test_turn_streams_retry_events() {
    // Create a provider that fails 3 times before succeeding
    // (initial attempt + 3 retries = 4 total attempts with RetryConfig::default())
    let flaky_provider = Arc::new(FlakyProvider::new(3));
    let provider = Arc::new(RetryProvider::new(flaky_provider, RetryConfig::default()));

    let (stream_tx, _stream_rx) = broadcast::channel(256);
    let (thread_event_tx, mut thread_event_rx) = broadcast::channel(256);

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
        .build()
        .unwrap();

    // Execute turn in background
    let turn_handle = tokio::spawn(async move { turn.execute().await });

    // Collect retry events from ThreadEvent::Processing
    let mut retry_count = 0;
    let mut max_retries_seen = 0;

    // Listen for events with a timeout
    let timeout = Duration::from_secs(5);
    let start = std::time::Instant::now();

    while start.elapsed() < timeout {
        match thread_event_rx.recv().await {
            Ok(ThreadEvent::Processing {
                event:
                    LlmStreamEvent::RetryAttempt {
                        attempt,
                        max_retries,
                        error,
                    },
                ..
            }) => {
                retry_count += 1;
                max_retries_seen = max_retries;
                assert_eq!(attempt, retry_count, "attempt numbers should be sequential");
                assert!(
                    error.contains("rate limited"),
                    "error should mention rate limiting"
                );
            }
            Ok(ThreadEvent::TurnCompleted { .. }) => {
                // Don't break, continue listening for more events
            }
            Ok(ThreadEvent::TurnFailed { .. }) => {
                panic!("turn should not fail with retryable errors");
            }
            Err(broadcast::error::RecvError::Lagged(_)) => {}
            Err(broadcast::error::RecvError::Closed) => break,
            _ => {}
        }
    }

    // Wait for turn to complete
    let turn_result = turn_handle.await.expect("turn task should complete");
    assert!(turn_result.is_ok(), "turn should succeed");

    // Should have retried 3 times
    assert_eq!(retry_count, 3, "should have 3 retry events");
    assert_eq!(
        max_retries_seen, 3,
        "should report max_retries=3 from RetryConfig::default()"
    );
}
