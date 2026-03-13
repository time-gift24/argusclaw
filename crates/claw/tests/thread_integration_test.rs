//! Integration tests for the thread module.
//!
//! These tests verify end-to-end thread execution including:
//! - Multi-turn conversation management
//! - Message history accumulation
//! - Context compaction
//! - Event broadcasting

use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::{Context, Poll};

use async_trait::async_trait;
use futures_core::Stream;
use rust_decimal::Decimal;

use claw::agents::compact::{Compactor, KeepRecentCompactor, KeepTokensCompactor};
use claw::agents::thread::{ThreadBuilder, ThreadConfigBuilder};
use claw::llm::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LlmError, LlmEventStream,
    LlmProvider, LlmStreamEvent, Role, ToolCallDelta, ToolCompletionRequest,
    ToolCompletionResponse, ToolDefinition,
};
use claw::tool::{NamedTool, ToolError, ToolManager};

// ============================================================================
// Mock Provider
// ============================================================================

/// Mock LLM provider that returns pre-defined responses in sequence.
struct SequentialMockProvider {
    responses: Mutex<Vec<ToolCompletionResponse>>,
    call_count: Mutex<usize>,
}

impl SequentialMockProvider {
    fn new(responses: Vec<ToolCompletionResponse>) -> Self {
        Self {
            responses: Mutex::new(responses),
            call_count: Mutex::new(0),
        }
    }
}

#[async_trait]
impl LlmProvider for SequentialMockProvider {
    fn model_name(&self) -> &str {
        "mock"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    fn context_window(&self) -> u32 {
        100_000
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        unimplemented!("complete not used in thread execution")
    }

    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        let mut count = self.call_count.lock().unwrap();
        let responses = self.responses.lock().unwrap();
        let response = responses
            .get(*count)
            .cloned()
            .unwrap_or_else(|| panic!("No more responses configured for call {}", count));
        *count += 1;
        Ok(response)
    }
}

struct EchoTool;

#[async_trait]
impl NamedTool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "echo".to_string(),
            description: "Echoes input back".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "message": { "type": "string" }
                }
            }),
        }
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        Ok(args)
    }
}

struct MockStream {
    events: Vec<LlmStreamEvent>,
    index: usize,
}

impl MockStream {
    fn new(events: Vec<LlmStreamEvent>) -> Self {
        Self { events, index: 0 }
    }
}

impl Stream for MockStream {
    type Item = Result<LlmStreamEvent, LlmError>;

    fn poll_next(mut self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if self.index >= self.events.len() {
            return Poll::Ready(None);
        }

        let item = self.events[self.index].clone();
        self.index += 1;
        Poll::Ready(Some(Ok(item)))
    }
}

struct StreamingToolLoopProvider {
    stream_calls: Mutex<usize>,
    tool_completion_calls: Mutex<usize>,
}

impl StreamingToolLoopProvider {
    fn new() -> Self {
        Self {
            stream_calls: Mutex::new(0),
            tool_completion_calls: Mutex::new(0),
        }
    }

    fn stream_call_count(&self) -> usize {
        *self.stream_calls.lock().unwrap()
    }

    fn tool_completion_call_count(&self) -> usize {
        *self.tool_completion_calls.lock().unwrap()
    }
}

#[async_trait]
impl LlmProvider for StreamingToolLoopProvider {
    fn model_name(&self) -> &str {
        "streaming-mock"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    fn context_window(&self) -> u32 {
        100_000
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        unreachable!("complete is not used in this test")
    }

    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        let mut calls = self.tool_completion_calls.lock().unwrap();
        let response = match *calls {
            0 => ToolCompletionResponse {
                content: Some("Done after tool".to_string()),
                reasoning_content: None,
                tool_calls: vec![],
                input_tokens: 25,
                output_tokens: 10,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
            n => panic!("unexpected complete_with_tools call {n}"),
        };
        *calls += 1;
        Ok(response)
    }

    async fn stream_complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        let mut calls = self.stream_calls.lock().unwrap();
        let events = match *calls {
            0 => vec![
                LlmStreamEvent::ToolCallDelta(ToolCallDelta {
                    index: 0,
                    id: Some("call_1".to_string()),
                    name: Some("echo".to_string()),
                    arguments_delta: Some(r#"{"message":"hello"}"#.to_string()),
                }),
                LlmStreamEvent::Usage {
                    input_tokens: 20,
                    output_tokens: 5,
                },
                LlmStreamEvent::Finished {
                    finish_reason: FinishReason::ToolUse,
                },
            ],
            n => panic!("unexpected stream call {n}"),
        };
        *calls += 1;
        Ok(Box::pin(MockStream::new(events)))
    }
}

// ============================================================================
// Helper Functions
// ============================================================================

fn create_simple_response(
    content: &str,
    input_tokens: u32,
    output_tokens: u32,
) -> ToolCompletionResponse {
    ToolCompletionResponse {
        content: Some(content.to_string()),
        reasoning_content: None,
        tool_calls: vec![],
        input_tokens,
        output_tokens,
        finish_reason: FinishReason::Stop,
        cache_read_input_tokens: 0,
        cache_creation_input_tokens: 0,
    }
}

// ============================================================================
// Integration Tests
// ============================================================================

#[tokio::test]
async fn test_thread_single_turn() {
    let responses = vec![create_simple_response("Hello! How can I help?", 50, 20)];

    let provider = Arc::new(SequentialMockProvider::new(responses));
    let compactor: Arc<dyn Compactor> = Arc::new(KeepRecentCompactor::with_defaults());

    let mut thread = ThreadBuilder::new()
        .provider(provider)
        .tool_manager(Arc::new(ToolManager::new()))
        .compactor(compactor)
        .build();

    // Subscribe to events
    let _event_rx = thread.subscribe();

    // Verify initial state before sending
    assert_eq!(thread.turn_count(), 0);

    // Send message
    let handle = thread.send_message("Hello".to_string()).await;

    // Turn count should be incremented after send_message
    assert_eq!(thread.turn_count(), 1);

    // Wait for completion
    let result = handle.wait_for_result().await;
    assert!(result.is_ok());
    assert_eq!(thread.history().len(), 2);
    assert_eq!(thread.history()[1].role, Role::Assistant);
    assert_eq!(thread.history()[1].content, "Hello! How can I help?");
    assert!(thread.token_count() > 0);
}

#[tokio::test]
async fn test_thread_multi_turn() {
    let responses = vec![
        create_simple_response("Hello!", 50, 10),
        create_simple_response("I'm doing well!", 60, 15),
        create_simple_response("Goodbye!", 70, 20),
    ];

    let provider = Arc::new(SequentialMockProvider::new(responses));
    let compactor: Arc<dyn Compactor> = Arc::new(KeepRecentCompactor::with_defaults());

    let mut thread = ThreadBuilder::new()
        .provider(provider)
        .tool_manager(Arc::new(ToolManager::new()))
        .compactor(compactor)
        .build();

    // Send three messages
    let handle1 = thread.send_message("Hello".to_string()).await;
    let _ = handle1.wait_for_result().await;

    let handle2 = thread.send_message("How are you?".to_string()).await;
    let _ = handle2.wait_for_result().await;

    let handle3 = thread.send_message("Goodbye".to_string()).await;
    let _ = handle3.wait_for_result().await;

    // Verify turn count
    assert_eq!(thread.turn_count(), 3);
}

#[tokio::test]
async fn test_thread_with_initial_history() {
    let responses = vec![create_simple_response("Continuing conversation", 80, 25)];

    let initial_messages = vec![
        ChatMessage::system("You are a helpful assistant"),
        ChatMessage::user("Previous question"),
        ChatMessage::assistant("Previous answer"),
    ];

    let provider = Arc::new(SequentialMockProvider::new(responses));
    let compactor: Arc<dyn Compactor> = Arc::new(KeepRecentCompactor::with_defaults());

    let mut thread = ThreadBuilder::new()
        .provider(provider)
        .tool_manager(Arc::new(ToolManager::new()))
        .compactor(compactor)
        .messages(initial_messages)
        .build();

    // Verify initial history
    assert_eq!(thread.history().len(), 3);

    // Send new message
    let handle = thread.send_message("New question".to_string()).await;
    let result = handle.wait_for_result().await;
    assert!(result.is_ok());

    // History should have grown
    assert!(thread.history().len() > 3);
    assert_eq!(thread.history().last().unwrap().role, Role::Assistant);
}

#[tokio::test]
async fn test_thread_streaming_tool_use_runs_until_model_stops() {
    let provider = Arc::new(StreamingToolLoopProvider::new());
    let compactor: Arc<dyn Compactor> = Arc::new(KeepRecentCompactor::with_defaults());
    let tool_manager = Arc::new(ToolManager::new());
    tool_manager.register(Arc::new(EchoTool));

    let mut thread = ThreadBuilder::new()
        .provider(provider.clone())
        .tool_manager(tool_manager)
        .compactor(compactor)
        .build();

    let handle = thread.send_message("Need tool".to_string()).await;
    let output = handle.wait_for_result().await.expect("turn should succeed");

    assert_eq!(provider.stream_call_count(), 1);
    assert_eq!(provider.tool_completion_call_count(), 1);
    assert!(
        output
            .messages
            .iter()
            .any(|msg| msg.role == Role::Tool && msg.tool_call_id.as_deref() == Some("call_1"))
    );
    assert_eq!(
        output.messages.last().map(|msg| msg.content.as_str()),
        Some("Done after tool")
    );
    assert_eq!(thread.history().len(), output.messages.len());
    assert_eq!(
        thread.history().last().map(|msg| msg.content.as_str()),
        Some("Done after tool")
    );
}

#[tokio::test]
async fn test_compact_preserves_system_messages() {
    let provider = Arc::new(SequentialMockProvider::new(vec![]));
    let compactor: Arc<dyn Compactor> = Arc::new(KeepRecentCompactor::new(0.8, 1));

    let mut thread = ThreadBuilder::new()
        .provider(provider)
        .tool_manager(Arc::new(ToolManager::new()))
        .compactor(compactor)
        .build();

    // Add multiple system messages
    thread
        .messages_mut()
        .push(ChatMessage::system("System prompt 1"));
    thread
        .messages_mut()
        .push(ChatMessage::system("System prompt 2"));

    // Add user messages
    for i in 1..=5 {
        thread
            .messages_mut()
            .push(ChatMessage::user(format!("User {}", i)));
    }

    // Force compact by setting high token count
    thread.set_token_count(100_000);

    // Compact
    let compactor = thread.compactor.clone();
    let result = compactor.compact(&mut thread).await;
    assert!(result.is_ok());

    // System messages should be preserved
    let system_count = thread
        .history()
        .iter()
        .filter(|m| m.role == Role::System)
        .count();
    assert_eq!(system_count, 2);

    // Only 1 recent non-system message
    let non_system_count = thread
        .history()
        .iter()
        .filter(|m| m.role != Role::System)
        .count();
    assert_eq!(non_system_count, 1);
}

#[tokio::test]
async fn test_compact_keep_tokens_strategy() {
    let provider = Arc::new(SequentialMockProvider::new(vec![]));
    let compactor: Arc<dyn Compactor> = Arc::new(KeepTokensCompactor::new(0.8, 0.5));

    let config = ThreadConfigBuilder::default()
        .build()
        .expect("config should build");

    let mut thread = ThreadBuilder::new()
        .provider(provider)
        .tool_manager(Arc::new(ToolManager::new()))
        .compactor(compactor)
        .config(config)
        .build();

    // Add system messages
    thread
        .messages_mut()
        .push(ChatMessage::system("System prompt"));

    // Add user messages
    for i in 1..=5 {
        thread
            .messages_mut()
            .push(ChatMessage::user(format!("User {}", i)));
    }

    // Force compact by setting high token count
    thread.set_token_count(100_000);

    // Compact
    let compactor = thread.compactor.clone();
    let result = compactor.compact(&mut thread).await;
    assert!(result.is_ok());

    // System messages should be preserved
    let system_count = thread
        .history()
        .iter()
        .filter(|m| m.role == Role::System)
        .count();
    assert_eq!(system_count, 1);
}
