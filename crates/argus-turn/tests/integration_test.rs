//! Integration tests for argus-turn

use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::broadcast;

use argus_llm::retry::{RetryConfig, RetryProvider};
use argus_protocol::AgentRecord;
use argus_protocol::ToolExecutionContext;
use argus_protocol::events::{ThreadControlEvent, ThreadEvent, ThreadJobResult, ThreadMailbox};
use argus_protocol::llm::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LlmError, LlmEventStream,
    LlmProvider, LlmStreamEvent, Role, ToolCall, ToolCompletionRequest, ToolCompletionResponse,
    ToolDefinition,
};
use argus_protocol::tool::{NamedTool, ToolError};
use argus_protocol::{AgentId, MessageOverride, ThreadId};
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

struct CaptureThreadIdTool {
    seen_thread_id: Arc<Mutex<Option<ThreadId>>>,
}

#[async_trait]
impl NamedTool for CaptureThreadIdTool {
    fn name(&self) -> &str {
        "capture_thread_id"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "capture_thread_id".to_string(),
            description: "Capture the routing thread id from the tool execution context"
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {}
            }),
        }
    }

    async fn execute(
        &self,
        _args: serde_json::Value,
        ctx: Arc<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError> {
        *self.seen_thread_id.lock().unwrap() = Some(ctx.thread_id);
        Ok(serde_json::json!({ "ok": true }))
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

#[test]
fn thread_mailbox_drains_interrupts_then_user_messages_then_job_results() {
    let mut mailbox = ThreadMailbox::default();

    mailbox.push(ThreadControlEvent::JobResult(ThreadJobResult {
        job_id: "job-1".to_string(),
        success: true,
        message: "first result".to_string(),
        token_usage: None,
        agent_id: AgentId::new(7),
        agent_display_name: "Researcher".to_string(),
        agent_description: "Finds background answers".to_string(),
    }));
    mailbox.push(ThreadControlEvent::UserMessage {
        content: "queued user follow-up".to_string(),
        msg_override: Some(MessageOverride::default()),
    });
    mailbox.push(ThreadControlEvent::UserInterrupt {
        content: "stop current path".to_string(),
    });
    mailbox.push(ThreadControlEvent::JobResult(ThreadJobResult {
        job_id: "job-2".to_string(),
        success: false,
        message: "second result".to_string(),
        token_usage: None,
        agent_id: AgentId::new(8),
        agent_display_name: "Builder".to_string(),
        agent_description: "Builds things".to_string(),
    }));

    let drained = mailbox.drain_for_turn();
    let rendered = drained
        .into_iter()
        .map(|item| item.into_message_text())
        .collect::<Vec<_>>();

    assert_eq!(rendered[0], "stop current path");
    assert_eq!(rendered[1], "queued user follow-up");
    assert!(rendered[2].contains("Job: job-1"));
    assert!(rendered[2].contains("Subagent: Researcher"));
    assert!(rendered[3].contains("Job: job-2"));
    assert!(mailbox.is_empty());
}

#[tokio::test]
async fn tool_execution_context_uses_originating_thread_id_for_nested_dispatch() {
    let originating_thread_id = ThreadId::new();
    let seen_thread_id = Arc::new(Mutex::new(None));
    let tool = Arc::new(CaptureThreadIdTool {
        seen_thread_id: Arc::clone(&seen_thread_id),
    });

    let provider = Arc::new(MockProvider::with_responses(vec![
        (
            String::new(),
            vec![ToolCall {
                id: "call-1".to_string(),
                name: "capture_thread_id".to_string(),
                arguments: serde_json::json!({}),
            }],
        ),
        ("done".to_string(), Vec::new()),
    ]));

    let (stream_tx, _) = broadcast::channel(32);
    let (thread_event_tx, _) = broadcast::channel(32);
    let (control_tx, _control_rx) = tokio::sync::mpsc::unbounded_channel();

    let turn = TurnBuilder::default()
        .turn_number(1)
        .thread_id("job-non-uuid".to_string())
        .originating_thread_id(originating_thread_id)
        .messages(vec![ChatMessage::user("run nested dispatch")])
        .provider(provider)
        .agent_record(Arc::new(AgentRecord::default()))
        .tools(vec![tool])
        .hooks(vec![])
        .config(TurnConfig::default())
        .stream_tx(stream_tx)
        .thread_event_tx(thread_event_tx)
        .control_tx(control_tx)
        .mailbox(Arc::new(tokio::sync::Mutex::new(ThreadMailbox::default())))
        .build()
        .unwrap();

    let result = turn.execute().await;
    assert!(result.is_ok(), "turn should complete");
    assert_eq!(
        *seen_thread_id.lock().unwrap(),
        Some(originating_thread_id),
        "tool context should preserve the originating thread route",
    );
}
