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

use claw::agents::thread::{
    CompactStrategy, ThreadBuilder, ThreadConfigBuilder, ThreadEvent,
};
use claw::llm::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LlmError, LlmProvider,
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

    async fn complete(
        &self,
        _request: CompletionRequest,
    ) -> Result<CompletionResponse, LlmError> {
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

fn create_simple_response(content: &str, input_tokens: u32, output_tokens: u32) -> ToolCompletionResponse {
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

    let mut thread = ThreadBuilder::new()
        .provider(provider)
        .tool_manager(Arc::new(ToolManager::new()))
        .build();

    // Subscribe to events
    let mut event_rx = thread.subscribe();

    // Send message
    let handle = thread.send_message("Hello".to_string()).await;

    // Verify initial state
    assert_eq!(thread.turn_count(), 1);

    // Wait for completion
    let result = handle.wait_for_result().await;
    assert!(result.is_ok());

    // Verify event was broadcast
    let event = event_rx.try_recv();
    assert!(event.is_ok());
    match event.unwrap() {
        ThreadEvent::TurnCompleted { turn_number, .. } => {
            assert_eq!(turn_number, 1);
        }
        _ => panic!("Expected TurnCompleted event"),
    }
}

#[tokio::test]
async fn test_thread_multi_turn_accumulates_history() {
    let responses = vec![
        create_simple_response("Response 1", 50, 20),
        create_simple_response("Response 2", 60, 25),
        create_simple_response("Response 3", 70, 30),
    ];

    let provider = Arc::new(SequentialMockProvider::new(responses));

    let mut thread = ThreadBuilder::new()
        .provider(provider)
        .tool_manager(Arc::new(ToolManager::new()))
        .build();

    // Execute three turns
    for i in 1..=3 {
        let handle = thread.send_message(format!("Message {}", i)).await;
        let result = handle.wait_for_result().await;
        assert!(result.is_ok(), "Turn {} failed", i);
    }

    // Verify turn count
    assert_eq!(thread.turn_count(), 3);

    // Verify history accumulated
    assert!(thread.history().len() >= 3);
}

#[tokio::test]
async fn test_thread_subscribe_receives_events() {
    let responses = vec![
        create_simple_response("Response 1", 50, 20),
        create_simple_response("Response 2", 60, 25),
    ];

    let provider = Arc::new(SequentialMockProvider::new(responses));

    let mut thread = ThreadBuilder::new()
        .provider(provider)
        .tool_manager(Arc::new(ToolManager::new()))
        .build();

    // Subscribe before sending messages
    let mut event_rx = thread.subscribe();

    // Execute two turns
    for i in 1..=2 {
        let handle = thread.send_message(format!("Message {}", i)).await;
        let _ = handle.wait_for_result().await;
    }

    // Collect events
    let mut events: Vec<ThreadEvent> = Vec::new();
    while let Ok(event) = event_rx.try_recv() {
        events.push(event);
    }

    // Should have TurnCompleted and Idle events for each turn
    assert!(events.len() >= 4); // 2 turns * (TurnCompleted + Idle)

    // Verify event types
    let completed_count = events
        .iter()
        .filter(|e| matches!(e, ThreadEvent::TurnCompleted { .. }))
        .count();
    assert_eq!(completed_count, 2);

    let idle_count = events
        .iter()
        .filter(|e| matches!(e, ThreadEvent::Idle { .. }))
        .count();
    assert_eq!(idle_count, 2);
}

#[tokio::test]
async fn test_thread_concurrent_subscribers() {
    let responses = vec![create_simple_response("Response", 50, 20)];

    let provider = Arc::new(SequentialMockProvider::new(responses));

    let mut thread = ThreadBuilder::new()
        .provider(provider)
        .tool_manager(Arc::new(ToolManager::new()))
        .build();

    // Multiple subscribers
    let mut rx1 = thread.subscribe();
    let mut rx2 = thread.subscribe();

    // Execute turn
    let handle = thread.send_message("Test".to_string()).await;
    let _ = handle.wait_for_result().await;

    // Both should receive events
    assert!(rx1.try_recv().is_ok());
    assert!(rx2.try_recv().is_ok());
}

#[tokio::test]
async fn test_compact_keep_recent() {
    let provider = Arc::new(SequentialMockProvider::new(vec![]));

    let config = ThreadConfigBuilder::default()
        .compact_strategy(CompactStrategy::KeepRecent { count: 2 })
        .build()
        .expect("config should build");

    let mut thread = ThreadBuilder::new()
        .provider(provider)
        .tool_manager(Arc::new(ToolManager::new()))
        .config(config)
        .build();

    // Add system message
    thread.messages.push(ChatMessage::system("System prompt"));

    // Add multiple user/assistant messages
    for i in 1..=5 {
        thread.messages.push(ChatMessage::user(format!("User {}", i)));
        thread.messages.push(ChatMessage::assistant(format!("Assistant {}", i)));
    }

    // Subscribe to capture compact event
    let mut event_rx = thread.subscribe();

    // Compact
    let result = thread.compact().await;
    assert!(result.is_ok());

    // Verify compacted event
    let event = event_rx.try_recv();
    assert!(matches!(event, Ok(ThreadEvent::Compacted { .. })));

    // Should have system + 2 recent non-system messages
    let non_system_count = thread.messages.iter().filter(|m| m.role != claw::llm::Role::System).count();
    assert_eq!(non_system_count, 2);
}

#[tokio::test]
async fn test_compact_keep_tokens() {
    let provider = Arc::new(SequentialMockProvider::new(vec![]));

    let config = ThreadConfigBuilder::default()
        .compact_strategy(CompactStrategy::KeepTokens { ratio: 0.5 })
        .build()
        .expect("config should build");

    let mut thread = ThreadBuilder::new()
        .provider(provider)
        .tool_manager(Arc::new(ToolManager::new()))
        .config(config)
        .build();

    // Add system message
    thread.messages.push(ChatMessage::system("System"));

    // Add messages with known token counts
    for i in 1..=10 {
        let content = "x".repeat(40); // ~10 tokens each
        thread.messages.push(ChatMessage::user(format!("{} {}", i, content)));
    }

    // Get initial token count (via public API)
    // Note: compact will recalculate internally
    let initial_token_count = thread.token_count();

    // Compact
    let result = thread.compact().await;
    assert!(result.is_ok());

    // Token count should be reduced or equal (if initial was already low)
    let new_token_count = thread.token_count();
    assert!(new_token_count <= initial_token_count || initial_token_count == 0);
}

#[tokio::test]
async fn test_thread_id_uniqueness() {
    let provider = Arc::new(SequentialMockProvider::new(vec![]));

    let thread1 = ThreadBuilder::new()
        .provider(provider.clone())
        .tool_manager(Arc::new(ToolManager::new()))
        .build();

    let thread2 = ThreadBuilder::new()
        .provider(provider)
        .tool_manager(Arc::new(ToolManager::new()))
        .build();

    assert_ne!(thread1.id, thread2.id);
}

#[tokio::test]
async fn test_thread_with_initial_history() {
    let responses = vec![create_simple_response("Continuing conversation", 80, 30)];

    let provider = Arc::new(SequentialMockProvider::new(responses));

    // Create thread with existing history
    let initial_messages = vec![
        ChatMessage::system("You are a helpful assistant"),
        ChatMessage::user("Previous question"),
        ChatMessage::assistant("Previous answer"),
    ];

    let mut thread = ThreadBuilder::new()
        .provider(provider)
        .tool_manager(Arc::new(ToolManager::new()))
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

    let config = ThreadConfigBuilder::default()
        .compact_strategy(CompactStrategy::KeepRecent { count: 1 })
        .build()
        .expect("config should build");

    let mut thread = ThreadBuilder::new()
        .provider(provider)
        .tool_manager(Arc::new(ToolManager::new()))
        .config(config)
        .build();

    // Add multiple system messages
    thread.messages.push(ChatMessage::system("System prompt 1"));
    thread.messages.push(ChatMessage::system("System prompt 2"));

    // Add user messages
    for i in 1..=5 {
        thread.messages.push(ChatMessage::user(format!("User {}", i)));
    }

    // Compact
    let result = thread.compact().await;
    assert!(result.is_ok());

    // System messages should be preserved
    let system_count = thread.messages.iter().filter(|m| m.role == claw::llm::Role::System).count();
    assert_eq!(system_count, 2);

    // Only 1 recent non-system message
    let non_system_count = thread.messages.iter().filter(|m| m.role != claw::llm::Role::System).count();
    assert_eq!(non_system_count, 1);
}
