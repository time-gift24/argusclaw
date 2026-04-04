//! Integration tests for argus-turn

use std::sync::{Arc, Mutex};
use std::time::Duration;

use chrono::Utc;
use tokio::sync::{Mutex as TokioMutex, broadcast, oneshot};

use argus_agent::turn::{TurnCancellation, TurnProgress};
use argus_agent::{TurnBuilder, TurnConfig};
use argus_llm::retry::{RetryConfig, RetryProvider};
use argus_protocol::AgentRecord;
use argus_protocol::ToolExecutionContext;
use argus_protocol::events::{
    MailboxMessage, MailboxMessageType, ThreadControlEvent, ThreadEvent, ThreadInbox,
    ThreadMailbox, TurnControlInput,
};
use argus_protocol::llm::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LlmError, LlmEventStream,
    LlmProvider, LlmStreamEvent, Role, ToolCall, ToolDefinition,
};
use argus_protocol::tool::{NamedTool, ToolError};
use argus_protocol::{AgentId, MessageOverride, ThreadId};
use argus_protocol::{
    ApprovalDecision, ApprovalRequest, ApprovalResponse, HookAction, HookEvent, HookHandler,
    RiskLevel, ToolHookContext,
};
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

fn sample_mailbox_message(job_id: &str) -> MailboxMessage {
    MailboxMessage {
        id: format!("msg-{job_id}"),
        from_thread_id: ThreadId::new(),
        to_thread_id: ThreadId::new(),
        from_label: "Worker".to_string(),
        message_type: MailboxMessageType::JobResult {
            job_id: job_id.to_string(),
            success: true,
            token_usage: None,
            agent_id: AgentId::new(42),
            agent_display_name: "Worker".to_string(),
            agent_description: "Background worker".to_string(),
        },
        text: format!("result for {job_id}"),
        timestamp: "2026-04-01T00:00:00Z".to_string(),
        read: false,
        summary: Some(format!("summary {job_id}")),
    }
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
        let finish_reason = if response.tool_calls.is_empty() {
            FinishReason::Stop
        } else {
            FinishReason::ToolUse
        };

        Ok(CompletionResponse {
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

struct BlockingApprovalHook {
    gate: Arc<TokioMutex<Option<oneshot::Receiver<()>>>>,
}

impl BlockingApprovalHook {
    fn new(gate: oneshot::Receiver<()>) -> Self {
        Self {
            gate: Arc::new(TokioMutex::new(Some(gate))),
        }
    }
}

#[async_trait]
impl HookHandler for BlockingApprovalHook {
    async fn on_tool_event(&self, ctx: &ToolHookContext) -> HookAction {
        if ctx.event != HookEvent::BeforeToolCall || ctx.tool_name != "echo" {
            return HookAction::Continue;
        }

        let request = ApprovalRequest::new(
            "test-agent".to_string(),
            ctx.tool_name.clone(),
            "pause for approval".to_string(),
            60,
            RiskLevel::High,
        );

        if let (Some(sender), Some(thread_id), Some(turn_number)) = (
            &ctx.thread_event_sender,
            ctx.thread_id.clone(),
            ctx.turn_number,
        ) {
            let _ = sender.send(ThreadEvent::WaitingForApproval {
                thread_id,
                turn_number,
                request: request.clone(),
            });
        }

        if let Some(gate) = self.gate.lock().await.take() {
            let _ = gate.await;
        }

        if let (Some(sender), Some(thread_id), Some(turn_number)) = (
            &ctx.thread_event_sender,
            ctx.thread_id.clone(),
            ctx.turn_number,
        ) {
            let response = ApprovalResponse {
                request_id: request.id,
                decision: ApprovalDecision::Approved,
                decided_at: Utc::now(),
                decided_by: Some("test".to_string()),
            };
            let _ = sender.send(ThreadEvent::ApprovalResolved {
                thread_id,
                turn_number,
                response,
            });
        }

        HookAction::Continue
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

        Ok(CompletionResponse {
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

    assert_eq!(output.appended_messages.len(), 1);
    assert_eq!(output.appended_messages[0].role, Role::Assistant);
    assert_eq!(output.appended_messages[0].content, "Hello, world!");
}

struct HangingStreamingProvider;

#[async_trait]
impl LlmProvider for HangingStreamingProvider {
    fn model_name(&self) -> &str {
        "hanging"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        Err(LlmError::UnsupportedCapability {
            provider: "hanging".to_string(),
            capability: "complete".to_string(),
        })
    }

    async fn stream_complete(
        &self,
        _request: CompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        let stream = futures_util::stream::unfold(0usize, |state| async move {
            tokio::time::sleep(Duration::from_millis(50)).await;
            Some((
                Ok(LlmStreamEvent::ContentDelta {
                    delta: format!("tick-{}", state),
                }),
                state.saturating_add(1),
            ))
        });

        Ok(Box::pin(stream))
    }
}

#[tokio::test]
async fn turn_cancel_returns_cancelled_instead_of_error() {
    let provider = Arc::new(HangingStreamingProvider);
    let cancellation = TurnCancellation::new();

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
        .cancellation(cancellation.clone())
        .build()
        .unwrap();

    let handle = tokio::spawn(async move { turn.execute().await });

    tokio::time::sleep(Duration::from_millis(80)).await;
    cancellation.cancel();

    let result = tokio::time::timeout(Duration::from_millis(300), handle)
        .await
        .expect("turn should terminate quickly after cancellation")
        .expect("turn task should not panic");

    assert!(matches!(result, Err(argus_agent::TurnError::Cancelled)));
}

#[tokio::test]
async fn turn_cancel_does_not_emit_terminal_thread_events() {
    let provider = Arc::new(HangingStreamingProvider);
    let cancellation = TurnCancellation::new();

    let (stream_tx, _) = broadcast::channel(256);
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
        .cancellation(cancellation.clone())
        .build()
        .unwrap();

    let handle = tokio::spawn(async move { turn.execute().await });

    tokio::time::sleep(Duration::from_millis(80)).await;
    cancellation.cancel();

    let result = tokio::time::timeout(Duration::from_millis(300), handle)
        .await
        .expect("turn should terminate quickly after cancellation")
        .expect("turn task should not panic");
    assert!(matches!(result, Err(argus_agent::TurnError::Cancelled)));

    let mut saw_failed = false;
    let mut saw_idle = false;
    let start = std::time::Instant::now();
    while start.elapsed() < Duration::from_millis(250) {
        match tokio::time::timeout(Duration::from_millis(50), thread_event_rx.recv()).await {
            Ok(Ok(ThreadEvent::Idle { .. })) => {
                saw_idle = true;
                break;
            }
            Ok(Ok(ThreadEvent::TurnFailed { .. })) => {
                saw_failed = true;
                break;
            }
            Ok(Ok(_)) => {}
            Ok(Err(broadcast::error::RecvError::Lagged(_))) => {}
            Ok(Err(broadcast::error::RecvError::Closed)) => break,
            Err(_) => {}
        }
    }

    assert!(
        !saw_failed,
        "raw turn execution should not emit terminal runtime lifecycle events"
    );
    assert!(
        !saw_idle,
        "raw turn execution should not emit Idle directly"
    );
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

    assert!(output.appended_messages.len() >= 3);
    // Should have tracked tokens
    assert!(output.token_usage.total_tokens > 0);
    // First assistant message should have tool calls
    let assistant_msgs: Vec<_> = output
        .appended_messages
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
    let mut saw_completed = false;

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
                saw_completed = true;
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
    assert!(
        !saw_completed,
        "raw turn execution should not emit TurnCompleted directly"
    );
}

#[test]
fn thread_inbox_drains_items_in_global_fifo_order() {
    let mut inbox = ThreadInbox::default();

    inbox.deliver_mailbox_message(MailboxMessage {
        id: "msg-job-1".to_string(),
        from_thread_id: ThreadId::new(),
        to_thread_id: ThreadId::new(),
        from_label: "Researcher".to_string(),
        message_type: MailboxMessageType::JobResult {
            job_id: "job-1".to_string(),
            success: true,
            token_usage: None,
            agent_id: AgentId::new(7),
            agent_display_name: "Researcher".to_string(),
            agent_description: "Finds background answers".to_string(),
        },
        text: "first result".to_string(),
        timestamp: "2026-04-01T00:00:00Z".to_string(),
        read: false,
        summary: None,
    });
    inbox.enqueue_user_message(
        "queued user follow-up".to_string(),
        Some(MessageOverride::default()),
    );
    inbox.deliver_mailbox_message(MailboxMessage {
        id: "msg-job-2".to_string(),
        from_thread_id: ThreadId::new(),
        to_thread_id: ThreadId::new(),
        from_label: "Builder".to_string(),
        message_type: MailboxMessageType::JobResult {
            job_id: "job-2".to_string(),
            success: false,
            token_usage: None,
            agent_id: AgentId::new(8),
            agent_display_name: "Builder".to_string(),
            agent_description: "Builds things".to_string(),
        },
        text: "second result".to_string(),
        timestamp: "2026-04-01T00:00:01Z".to_string(),
        read: false,
        summary: None,
    });

    let drained = inbox.drain_for_turn();
    let rendered = drained
        .iter()
        .cloned()
        .map(TurnControlInput::into_message_text)
        .collect::<Vec<_>>();

    assert_eq!(rendered.len(), 3);
    assert!(rendered[0].contains("Job: job-1"));
    assert_eq!(rendered[1], "queued user follow-up");
    assert!(rendered[0].contains("Subagent: Researcher"));
    assert!(rendered[2].contains("Job: job-2"));
    let msg_override = match &drained[1] {
        TurnControlInput::UserMessage { msg_override, .. } => msg_override.clone(),
        _ => panic!("second drained item should be a user message"),
    };
    assert!(
        msg_override.is_some(),
        "msg_override should survive inbox queueing",
    );
    assert!(inbox.is_empty());
}

#[test]
fn thread_mailbox_preserves_fifo_user_messages() {
    let mut mailbox = ThreadMailbox::default();

    mailbox.enqueue_user_message("first user message".to_string(), None);
    mailbox.enqueue_user_message(
        "second user message".to_string(),
        Some(MessageOverride::default()),
    );
    mailbox.enqueue_mailbox_message(sample_mailbox_message("job-3"));

    let drained = mailbox.drain_for_turn();
    let rendered = drained
        .into_iter()
        .map(|item| item.into_message_text())
        .collect::<Vec<_>>();

    assert_eq!(rendered.len(), 3);
    assert_eq!(rendered[0], "first user message");
    assert_eq!(rendered[1], "second user message");
    assert!(rendered[2].contains("Job: job-3"));
}

#[test]
fn thread_mailbox_take_next_turn_message_preserves_global_fifo() {
    let mut mailbox = ThreadMailbox::default();
    mailbox.enqueue_user_message("first".to_string(), None);
    mailbox.enqueue_mailbox_message(sample_mailbox_message("job-1"));

    let first = mailbox
        .take_next_turn_message()
        .expect("first queued message should exist");
    let second = mailbox
        .take_next_turn_message()
        .expect("second queued message should exist");

    assert_eq!(first.content, "first");
    assert!(second.content.contains("Job: job-1"));
}

#[test]
fn thread_mailbox_interrupt_stop_is_not_enqueued() {
    let mut mailbox = ThreadMailbox::default();
    mailbox.interrupt_stop();

    assert!(mailbox.take_next_turn_message().is_none());
    assert!(mailbox.take_stop_signal());
    assert!(!mailbox.take_stop_signal());
}

#[test]
fn thread_mailbox_legacy_interrupts_drain_before_inbox_items() {
    let mut mailbox = ThreadMailbox::default();

    mailbox.push(ThreadControlEvent::UserMessage {
        content: "follow-up".to_string(),
        msg_override: None,
    });
    mailbox.push(ThreadControlEvent::UserInterrupt {
        content: "interrupt now".to_string(),
    });
    mailbox.push(ThreadControlEvent::DeliverMailboxMessage(
        sample_mailbox_message("job-legacy"),
    ));

    let drained = mailbox
        .drain_for_turn()
        .into_iter()
        .map(TurnControlInput::into_message_text)
        .collect::<Vec<_>>();

    assert_eq!(drained.len(), 3);
    assert_eq!(drained[0], "interrupt now");
    assert_eq!(drained[1], "follow-up");
    assert!(drained[2].contains("Job: job-legacy"));
}

#[test]
fn thread_mailbox_idle_handoff_clears_legacy_interrupts_without_turning_them_into_next_turn_input()
{
    let mut mailbox = ThreadMailbox::default();

    mailbox.push(ThreadControlEvent::UserInterrupt {
        content: "interrupt-before-idle".to_string(),
    });
    mailbox.push(ThreadControlEvent::UserMessage {
        content: "first-user".to_string(),
        msg_override: Some(MessageOverride::default()),
    });
    mailbox.push(ThreadControlEvent::DeliverMailboxMessage(
        sample_mailbox_message("job-handoff-mailbox"),
    ));
    mailbox.push(ThreadControlEvent::UserInterrupt {
        content: "interrupt-after-work".to_string(),
    });

    let first = mailbox
        .take_next_turn_message()
        .expect("first handoff item should exist");
    let second = mailbox
        .take_next_turn_message()
        .expect("second handoff item should exist");

    assert_eq!(first.content, "first-user");
    assert!(
        first.msg_override.is_some(),
        "compatibility path should preserve msg_override",
    );
    assert!(
        second.content.contains("Job: job-handoff-mailbox"),
        "queued work should preserve FIFO order for idle handoff",
    );
    assert!(
        second.msg_override.is_none(),
        "job handoff should not carry user msg_override",
    );
    assert!(mailbox.take_next_turn_message().is_none());
    assert!(
        mailbox.drain_for_turn().is_empty(),
        "interrupts should be cleared when idling into handoff",
    );
}

#[test]
fn thread_inbox_take_next_turn_message_follows_global_fifo_handoff() {
    let mut inbox = ThreadInbox::default();

    inbox.deliver_mailbox_message(MailboxMessage {
        id: "msg-job-handoff".to_string(),
        from_thread_id: ThreadId::new(),
        to_thread_id: ThreadId::new(),
        from_label: "Handoff".to_string(),
        message_type: MailboxMessageType::JobResult {
            job_id: "job-handoff".to_string(),
            success: true,
            token_usage: None,
            agent_id: AgentId::new(11),
            agent_display_name: "Handoff".to_string(),
            agent_description: "Idle handoff ordering".to_string(),
        },
        text: "job-first".to_string(),
        timestamp: "2026-04-01T00:00:05Z".to_string(),
        read: false,
        summary: None,
    });
    inbox.enqueue_user_message("user-second".to_string(), Some(MessageOverride::default()));

    let first = inbox
        .take_next_turn_message()
        .expect("first handoff item should exist");
    let second = inbox
        .take_next_turn_message()
        .expect("second handoff item should exist");

    assert!(
        first.content.contains("Job: job-handoff"),
        "job result should be handed off first in global FIFO",
    );
    assert!(first.msg_override.is_none());
    assert_eq!(second.content, "user-second");
    assert!(
        second.msg_override.is_some(),
        "msg_override should survive idle handoff",
    );
    assert!(inbox.take_next_turn_message().is_none());
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

#[tokio::test]
async fn turn_progress_approval_pause_and_resume_emits_progress_and_thread_events() {
    let provider = Arc::new(MockProvider::with_responses(vec![
        (
            "first".to_string(),
            vec![ToolCall {
                id: "call-approval".to_string(),
                name: "echo".to_string(),
                arguments: serde_json::json!({"message": "approval test"}),
            }],
        ),
        ("done".to_string(), Vec::new()),
    ]));

    let (resume_tx, resume_rx) = oneshot::channel();
    let approval_hook = Arc::new(BlockingApprovalHook::new(resume_rx));
    let (stream_tx, _) = broadcast::channel(256);
    let (thread_event_tx, mut thread_event_rx) = broadcast::channel(256);

    let turn = TurnBuilder::default()
        .turn_number(1)
        .thread_id("integration-turn".to_string())
        .messages(vec![ChatMessage::user("start")])
        .provider(provider)
        .agent_record(Arc::new(AgentRecord::default()))
        .tools(vec![Arc::new(EchoTool)])
        .hooks(vec![approval_hook])
        .config(TurnConfig {
            max_iterations: Some(5),
            ..TurnConfig::default()
        })
        .stream_tx(stream_tx)
        .thread_event_tx(thread_event_tx)
        .build()
        .unwrap();

    let execution = turn.execute_progress();

    let mut saw_waiting = false;
    while !saw_waiting {
        match tokio::time::timeout(Duration::from_secs(1), thread_event_rx.recv())
            .await
            .expect("should receive approval event in time")
        {
            Ok(ThreadEvent::WaitingForApproval { .. }) => saw_waiting = true,
            Ok(_) => {}
            Err(broadcast::error::RecvError::Lagged(_)) => {}
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }

    resume_tx
        .send(())
        .expect("approval hook should still be waiting");

    let execution = tokio::time::timeout(Duration::from_secs(2), execution.collect())
        .await
        .expect("turn should resume and finish");

    assert!(execution.result.is_ok());
    assert!(
        execution
            .progress
            .iter()
            .any(|item| matches!(item, TurnProgress::WaitingForApproval { .. }))
    );
    assert!(
        execution
            .progress
            .iter()
            .any(|item| matches!(item, TurnProgress::ApprovalResolved { .. }))
    );
}
