//! Thread implementation.

use std::sync::Arc;

use derive_builder::Builder;
use tokio::sync::broadcast;

use argus_protocol::llm::{ChatMessage, LlmProvider};
use argus_protocol::{HookRegistry, ThreadEvent};
use argus_tool::ToolManager;
use argus_turn::{TurnInputBuilder, TurnOutput, TurnStreamEvent, execute_turn_streaming};

use super::compact::{CompactContext, Compactor};
use super::config::ThreadConfig;
use super::error::ThreadError;
use super::types::{ThreadInfo, ThreadState};

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
    #[builder(default = "uuid::Uuid::new_v4().to_string()")]
    id: String,

    /// Initial message history (for restoring sessions).
    #[builder(default)]
    messages: Vec<ChatMessage>,

    /// LLM provider (required).
    provider: Arc<dyn LlmProvider>,

    /// Tool manager.
    #[builder(default = "Arc::new(ToolManager::new())")]
    tool_manager: Arc<ToolManager>,

    /// Compactor for managing context size.
    compactor: Arc<dyn Compactor>,

    /// Hook registry for lifecycle events (optional).
    #[builder(default, setter(strip_option))]
    hooks: Option<Arc<HookRegistry>>,

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
            id: self.id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            messages: self.messages.unwrap_or_default(),
            provider: self.provider.ok_or(ThreadError::ProviderNotConfigured)?,
            tool_manager: self
                .tool_manager
                .unwrap_or_else(|| Arc::new(ToolManager::new())),
            compactor: self.compactor.ok_or(ThreadError::CompactorNotConfigured)?,
            hooks: self.hooks.flatten(),
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
        config: ThreadConfig,
    ) -> Self {
        let (event_sender, _) = broadcast::channel(DEFAULT_CHANNEL_CAPACITY);

        Self {
            id: uuid::Uuid::new_v4().to_string(),
            messages: Vec::new(),
            provider,
            tool_manager,
            compactor,
            hooks: None,
            config,
            token_count: 0,
            turn_count: 0,
            event_sender,
        }
    }

    /// Get the Thread ID.
    pub fn id(&self) -> &str {
        &self.id
    }

    /// Get information about this thread.
    pub fn info(&self) -> ThreadInfo {
        ThreadInfo {
            id: self.id.clone(),
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

        // Create CompactContext for compaction
        {
            let mut context =
                CompactContext::new(&self.provider, &mut self.token_count, &mut self.messages);
            if let Err(e) = compactor.compact(&mut context).await {
                tracing::warn!("Compact failed: {}", e);
            }
        }

        self.messages.push(ChatMessage::user(user_input));
        self.recalculate_token_count();
        self.execute_turn_streaming().await
    }

    async fn execute_turn_streaming(&mut self) -> Result<(), ThreadError> {
        self.turn_count += 1;
        let turn_number = self.turn_count;
        let thread_id = self.id.clone();

        // Create channel for streaming events
        let (stream_tx, mut stream_rx) = broadcast::channel(DEFAULT_CHANNEL_CAPACITY);

        // Build TurnInput with stream_sender
        let tool_ids = self.tool_manager.list_ids();
        let mut turn_input_builder = TurnInputBuilder::new()
            .provider(self.provider.clone())
            .messages(self.messages.clone())
            .tool_manager(self.tool_manager.clone())
            .tool_ids(tool_ids)
            .thread_event_sender(self.event_sender.clone())
            .thread_id(thread_id.clone())
            .stream_sender(stream_tx);

        // Set hooks if present
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
        let forwarder_thread_id = thread_id.clone();
        let forwarder = tokio::spawn(async move {
            while let Ok(event) = stream_rx.recv().await {
                match event {
                    TurnStreamEvent::LlmEvent(llm_event) => {
                        let _ = forwarder_event_sender.send(ThreadEvent::Processing {
                            thread_id: forwarder_thread_id.clone(),
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
                            thread_id: forwarder_thread_id.clone(),
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
                            thread_id: forwarder_thread_id.clone(),
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
                    thread_id: thread_id.clone(),
                    turn_number,
                    token_usage,
                });
                Ok(())
            }
            Err(error) => {
                let _ = event_sender.send(ThreadEvent::TurnFailed {
                    thread_id: thread_id.clone(),
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
    use crate::compact::KeepRecentCompactor;

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
}
