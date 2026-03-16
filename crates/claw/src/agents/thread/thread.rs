//! Thread implementation.

use std::sync::Arc;

use derive_builder::Builder;
use tokio::sync::broadcast;

use crate::agents::compact::Compactor;
use crate::agents::turn::{TurnInputBuilder, TurnOutput, TurnStreamEvent, execute_turn_streaming};
use crate::approval::ApprovalManager;
use crate::llm::{ChatMessage, LlmProvider};
use crate::protocol::HookRegistry;
use crate::tool::ToolManager;

use super::{ThreadConfig, ThreadError, ThreadInfo, ThreadState};
use crate::protocol::{ThreadEvent, ThreadId};

/// Default broadcast channel capacity.
const DEFAULT_CHANNEL_CAPACITY: usize = 256;

/// Thread - multi-turn conversation session.
///
/// A Thread manages message history and executes Turns sequentially.
/// It broadcasts events to subscribers for real-time updates.
#[derive(Builder)]
#[builder(pattern = "owned", build_fn(skip))]
pub struct Thread {
    /// Unique identifier.
    #[builder(default = ThreadId::new())]
    id: ThreadId,

    /// Initial message history (for restoring sessions).
    #[builder(default)]
    messages: Vec<ChatMessage>,

    /// LLM provider (required).
    provider: Arc<dyn LlmProvider>,

    /// Tool manager.
    #[builder(default = "Arc::new(ToolManager::new())")]
    tool_manager: Arc<ToolManager>,

    /// Compactor for managing context size.
    pub(crate) compactor: Arc<dyn Compactor>,

    /// Approval manager (optional, used by approval hooks via Arc sharing).
    #[builder(default, setter(strip_option))]
    #[allow(dead_code)]
    approval_manager: Option<Arc<ApprovalManager>>,

    /// Hook registry for lifecycle events (optional).
    #[builder(default, setter(strip_option))]
    hooks: Option<Arc<HookRegistry>>,

    /// Tool names to use (empty = all tools).
    #[builder(default)]
    tool_names: Option<Vec<String>>,

    /// Thread configuration.
    #[builder(default)]
    config: ThreadConfig,

    /// Token count (internal).
    #[builder(default)]
    token_count: u32,

    /// Turn count (internal).
    #[builder(default)]
    turn_count: u32,

    /// Event broadcaster (internal).
    #[builder(default)]
    event_sender: broadcast::Sender<ThreadEvent>,
}

impl std::fmt::Debug for Thread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Thread")
            .field("id", &self.id)
            .field("messages", &self.messages.len())
            .field("token_count", &self.token_count)
            .field("turn_count", &self.turn_count)
            .field("config", &self.config)
            .finish()
    }
}

impl ThreadBuilder {
    /// Create a new ThreadBuilder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build the Thread.
    ///
    /// # Errors
    ///
    /// Returns `ThreadError` if required fields (`provider`, `compactor`) are not set.
    pub fn build(self) -> Result<Thread, ThreadError> {
        let (event_sender, _) = broadcast::channel(DEFAULT_CHANNEL_CAPACITY);

        Ok(Thread {
            id: self.id.unwrap_or_default(),
            messages: self.messages.unwrap_or_default(),
            provider: self.provider.ok_or(ThreadError::ProviderNotConfigured)?,
            tool_manager: self
                .tool_manager
                .unwrap_or_else(|| Arc::new(ToolManager::new())),
            compactor: self.compactor.ok_or(ThreadError::CompactorNotConfigured)?,
            approval_manager: self.approval_manager.flatten(),
            hooks: self.hooks.flatten(),
            tool_names: self.tool_names.flatten(),
            config: self.config.unwrap_or_default(),
            token_count: 0,
            turn_count: 0,
            event_sender,
        })
    }
}

impl Thread {
    /// Create a new Thread with the given provider and configuration.
    ///
    /// This is a convenience method that creates a Thread with default settings.
    /// For more control, use `ThreadBuilder`.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        provider: Arc<dyn LlmProvider>,
        tool_manager: Arc<ToolManager>,
        compactor: Arc<dyn Compactor>,
        approval_manager: Option<Arc<ApprovalManager>>,
        config: ThreadConfig,
    ) -> Self {
        let (event_sender, _) = broadcast::channel(DEFAULT_CHANNEL_CAPACITY);

        Self {
            id: ThreadId::new(),
            messages: Vec::new(),
            provider,
            tool_manager,
            compactor,
            approval_manager,
            hooks: None,
            tool_names: None,
            config,
            token_count: 0,
            turn_count: 0,
            event_sender,
        }
    }

    /// Get the Thread ID.
    pub fn id(&self) -> &ThreadId {
        &self.id
    }

    /// Get information about this thread.
    pub fn info(&self) -> ThreadInfo {
        ThreadInfo {
            id: self.id,
            message_count: self.messages.len(),
            token_count: self.token_count,
            turn_count: self.turn_count,
        }
    }

    /// Subscribe to Thread events.
    ///
    /// Multiple subscribers can receive events simultaneously.
    pub fn subscribe(&self) -> broadcast::Receiver<ThreadEvent> {
        self.event_sender.subscribe()
    }

    /// Get current state.
    pub fn state(&self) -> ThreadState {
        ThreadState::Idle
    }

    /// Get message history (read-only).
    pub fn history(&self) -> &[ChatMessage] {
        &self.messages
    }

    /// Get current token count.
    pub fn token_count(&self) -> u32 {
        self.token_count
    }

    /// Get turn count.
    pub fn turn_count(&self) -> u32 {
        self.turn_count
    }

    /// Get the LLM provider.
    pub fn provider(&self) -> &Arc<dyn LlmProvider> {
        &self.provider
    }

    /// Get mutable access to messages (for Compactor).
    pub fn messages_mut(&mut self) -> &mut Vec<ChatMessage> {
        &mut self.messages
    }

    /// Set the token count (for Compactor).
    pub fn set_token_count(&mut self, count: u32) {
        self.token_count = count;
    }

    /// Recalculate token count from messages.
    pub fn recalculate_token_count(&mut self) {
        self.token_count = self
            .messages
            .iter()
            .map(|m| Self::estimate_tokens(&m.content))
            .sum();
    }

    fn apply_turn_output(&mut self, output: TurnOutput) {
        self.messages = output.messages;
        self.recalculate_token_count();
    }

    /// Send user message and execute Turn.
    pub async fn send_message(&mut self, user_input: String) -> Result<(), ThreadError> {
        // Compactor decides internally whether to compact
        // Clone the Arc first to avoid borrow conflicts
        let compactor = self.compactor.clone();
        if let Err(e) = compactor.compact(self).await {
            tracing::warn!("Compact failed: {}", e);
        }

        self.messages.push(ChatMessage::user(user_input));
        self.recalculate_token_count();
        self.execute_turn_streaming().await
    }

    async fn execute_turn_streaming(&mut self) -> Result<(), ThreadError> {
        self.turn_count += 1;
        let turn_number = self.turn_count;
        let thread_id = self.id;

        // Create channel for streaming events
        let (stream_tx, mut stream_rx) = broadcast::channel(DEFAULT_CHANNEL_CAPACITY);

        // Build TurnInput with stream_sender
        // Use configured tool_names if present, otherwise use all available tools
        let tool_ids = match &self.tool_names {
            Some(names) if !names.is_empty() => names.clone(),
            _ => self.tool_manager.list_ids(),
        };
        let mut turn_input_builder = TurnInputBuilder::new()
            .provider(self.provider.clone())
            .messages(self.messages.clone())
            .tool_manager(self.tool_manager.clone())
            .tool_ids(tool_ids)
            .thread_event_sender(self.event_sender.clone())
            .thread_id(thread_id)
            .stream_sender(stream_tx);

        // Set hooks if present (need to use into_option pattern for strip_option)
        if let Some(hooks) = self.hooks.clone() {
            turn_input_builder = turn_input_builder.hooks(hooks);
        }

        // SAFETY: provider is always set since Thread requires it at construction.
        let turn_input = turn_input_builder
            .build()
            .expect("TurnInput build cannot fail: provider is guaranteed by Thread");

        let event_sender = self.event_sender.clone();
        let config = self.config.turn_config.clone();

        // Start event forwarding task (runs concurrently with turn execution)
        let forwarder_event_sender = event_sender.clone();
        let forwarder = tokio::spawn(async move {
            while let Ok(event) = stream_rx.recv().await {
                match event {
                    TurnStreamEvent::LlmEvent(llm_event) => {
                        let _ = forwarder_event_sender.send(ThreadEvent::Processing {
                            thread_id,
                            turn_number,
                            event: llm_event,
                        });
                    }
                    TurnStreamEvent::ToolStarted {
                        tool_call_id,
                        tool_name,
                        arguments,
                    } => {
                        let _ = forwarder_event_sender.send(ThreadEvent::ToolStarted {
                            thread_id,
                            turn_number,
                            tool_call_id,
                            tool_name,
                            arguments,
                        });
                    }
                    TurnStreamEvent::ToolCompleted {
                        tool_call_id,
                        tool_name,
                        result: tool_result,
                    } => {
                        let _ = forwarder_event_sender.send(ThreadEvent::ToolCompleted {
                            thread_id,
                            turn_number,
                            tool_call_id,
                            tool_name,
                            result: tool_result,
                        });
                    }
                }
            }
        });

        let result = execute_turn_streaming(turn_input, config).await;
        let _ = forwarder.await;

        let final_result = match result {
            Ok(output) => {
                let token_usage = output.token_usage.clone();
                self.apply_turn_output(output);
                let _ = event_sender.send(ThreadEvent::TurnCompleted {
                    thread_id,
                    turn_number,
                    token_usage,
                });
                Ok(())
            }
            Err(error) => {
                let _ = event_sender.send(ThreadEvent::TurnFailed {
                    thread_id,
                    turn_number,
                    error: error.to_string(),
                });
                Err(ThreadError::TurnFailed(error))
            }
        };

        let _ = event_sender.send(ThreadEvent::Idle { thread_id });
        final_result
    }

    /// Estimate token count for a string.
    fn estimate_tokens(content: &str) -> u32 {
        (content.len() / 4).max(1) as u32
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agents::compact::KeepRecentCompactor;

    #[test]
    fn thread_builder_requires_provider() {
        let compactor: Arc<dyn Compactor> = Arc::new(KeepRecentCompactor::with_defaults());
        let result = ThreadBuilder::new().compactor(compactor).build();
        assert!(matches!(result, Err(ThreadError::ProviderNotConfigured)));
    }

    #[test]
    fn thread_builder_requires_compactor() {
        let result = ThreadBuilder::new().build();
        assert!(result.is_err());
    }

    #[test]
    fn estimate_tokens_reasonable() {
        assert_eq!(Thread::estimate_tokens("test"), 1);
        assert_eq!(Thread::estimate_tokens("test test"), 2);
        assert_eq!(Thread::estimate_tokens(""), 1);
    }

    // ========================================================================
    // Integration tests (migrated from tests/thread_integration_test.rs)
    // ========================================================================

    use std::sync::Mutex;

    use async_trait::async_trait;
    use rust_decimal::Decimal;

    use crate::agents::compact::{Compactor, KeepTokensCompactor};
    use crate::llm::provider::{CompletionRequest, CompletionResponse};
    use crate::llm::{
        FinishReason, LlmError, LlmProvider, Role, ToolCompletionRequest, ToolCompletionResponse,
    };

    use super::super::ThreadConfigBuilder;

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

    #[tokio::test]
    async fn test_thread_single_turn() {
        let responses = vec![create_simple_response("Hello! How can I help?", 50, 20)];

        let provider = Arc::new(SequentialMockProvider::new(responses));
        let compactor: Arc<dyn Compactor> = Arc::new(KeepRecentCompactor::with_defaults());

        let mut thread = ThreadBuilder::new()
            .provider(provider)
            .tool_manager(Arc::new(ToolManager::new()))
            .compactor(compactor)
            .build()
            .unwrap();

        let _event_rx = thread.subscribe();

        assert_eq!(thread.turn_count(), 0);

        let result = thread.send_message("Hello".to_string()).await;

        assert_eq!(thread.turn_count(), 1);

        assert!(result.is_ok());
        assert_eq!(thread.history().len(), 2);
        assert_eq!(thread.history()[0].role, Role::User);
        assert_eq!(thread.history()[0].content, "Hello");
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
            .build()
            .unwrap();

        let _ = thread.send_message("Hello".to_string()).await;

        let _ = thread.send_message("How are you?".to_string()).await;

        let _ = thread.send_message("Goodbye".to_string()).await;

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
            .build()
            .unwrap();

        assert_eq!(thread.history().len(), 3);

        let result = thread.send_message("New question".to_string()).await;
        assert!(result.is_ok());

        assert!(thread.history().len() > 3);
        assert_eq!(
            thread
                .history()
                .last()
                .map(|message| message.content.as_str()),
            Some("Continuing conversation")
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
            .build()
            .unwrap();

        thread
            .messages_mut()
            .push(ChatMessage::system("System prompt 1"));
        thread
            .messages_mut()
            .push(ChatMessage::system("System prompt 2"));

        for i in 1..=5 {
            thread
                .messages_mut()
                .push(ChatMessage::user(format!("User {}", i)));
        }

        thread.set_token_count(100_000);

        let compactor = thread.compactor.clone();
        let result = compactor.compact(&mut thread).await;
        assert!(result.is_ok());

        let system_count = thread
            .history()
            .iter()
            .filter(|m| m.role == Role::System)
            .count();
        assert_eq!(system_count, 2);

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
            .build()
            .unwrap();

        thread
            .messages_mut()
            .push(ChatMessage::system("System prompt"));

        for i in 1..=5 {
            thread
                .messages_mut()
                .push(ChatMessage::user(format!("User {}", i)));
        }

        thread.set_token_count(100_000);

        let compactor = thread.compactor.clone();
        let result = compactor.compact(&mut thread).await;
        assert!(result.is_ok());

        let system_count = thread
            .history()
            .iter()
            .filter(|m| m.role == Role::System)
            .count();
        assert_eq!(system_count, 1);
    }
}
