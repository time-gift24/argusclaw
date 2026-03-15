#![cfg(feature = "dev")]
//! Integration tests for the thread module.
//!
//! These tests verify end-to-end thread execution including:
//! - Multi-turn conversation management
//! - Message history accumulation
//! - Context compaction
//! - Event broadcasting

use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use rust_decimal::Decimal;

use claw::agents::compact::{Compactor, KeepRecentCompactor, KeepTokensCompactor};
use claw::agents::thread::{ThreadBuilder, ThreadConfigBuilder};
use claw::llm::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LlmError, LlmProvider, Role,
    ToolCompletionRequest, ToolCompletionResponse,
};
use claw::tool::ToolManager;

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
