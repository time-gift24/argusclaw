//! Thread implementation.

use std::sync::Arc;

use derive_builder::Builder;
use tokio::sync::{broadcast, oneshot};

use crate::agents::compact::Compactor;
use crate::agents::turn::{
    TurnError, TurnInputBuilder, TurnOutput, TurnStreamEvent, execute_turn_streaming,
};
use crate::approval::ApprovalManager;
use crate::llm::{ChatMessage, LlmProvider, LlmStreamEvent};
use crate::protocol::HookRegistry;
use crate::tool::ToolManager;

use super::{ThreadConfig, ThreadError, ThreadEvent, ThreadId, ThreadInfo, ThreadState};

/// Default broadcast channel capacity.
const DEFAULT_CHANNEL_CAPACITY: usize = 256;

/// Handle for receiving Turn execution events.
pub struct TurnStreamHandle {
    /// Thread ID.
    pub thread_id: ThreadId,
    /// Turn number.
    pub turn_number: u32,
    /// Raw LLM events during processing.
    pub llm_events: broadcast::Receiver<LlmStreamEvent>,
    /// Final result when Turn completes.
    pub result: oneshot::Receiver<Result<TurnOutput, TurnError>>,
}

impl TurnStreamHandle {
    /// Wait for Turn completion and get the result.
    pub async fn wait_for_result(self) -> Result<TurnOutput, ThreadError> {
        self.result
            .await
            .map_err(|_| ThreadError::ChannelClosed)?
            .map_err(ThreadError::TurnFailed)
    }
}

/// Thread - multi-turn conversation session.
///
/// A Thread manages message history and executes Turns sequentially.
/// It broadcasts events to subscribers for real-time updates.
#[derive(Builder)]
#[builder(pattern = "owned", build_fn(skip))]
pub struct Thread {
    /// Unique identifier.
    #[builder(default = ThreadId::new())]
    pub id: ThreadId,

    /// Initial message history (for restoring sessions).
    #[builder(default)]
    pub messages: Vec<ChatMessage>,

    /// LLM provider (required).
    pub provider: Arc<dyn LlmProvider>,

    /// Tool manager.
    #[builder(default = "Arc::new(ToolManager::new())")]
    pub tool_manager: Arc<ToolManager>,

    /// Compactor for managing context size.
    pub compactor: Arc<dyn Compactor>,

    /// Approval manager (optional).
    #[builder(default, setter(strip_option))]
    pub approval_manager: Option<Arc<ApprovalManager>>,

    /// Hook registry for lifecycle events (optional).
    #[builder(default, setter(strip_option))]
    pub hooks: Option<Arc<HookRegistry>>,

    /// Thread configuration.
    #[builder(default)]
    pub config: ThreadConfig,

    /// Token count (internal).
    #[builder(default)]
    pub(super) token_count: u32,

    /// Turn count (internal).
    #[builder(default)]
    pub(super) turn_count: u32,

    /// Event broadcaster (internal).
    #[builder(default)]
    pub(super) event_sender: broadcast::Sender<ThreadEvent>,
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
    /// # Panics
    ///
    /// Panics if provider or compactor is not set.
    #[must_use]
    pub fn build(self) -> Thread {
        let (event_sender, _) = broadcast::channel(DEFAULT_CHANNEL_CAPACITY);

        Thread {
            id: self.id.unwrap_or_default(),
            messages: self.messages.unwrap_or_default(),
            provider: self.provider.expect("provider is required"),
            tool_manager: self
                .tool_manager
                .unwrap_or_else(|| Arc::new(ToolManager::new())),
            compactor: self.compactor.expect("compactor is required"),
            approval_manager: self.approval_manager.flatten(),
            hooks: self.hooks.flatten(),
            config: self.config.unwrap_or_default(),
            token_count: 0,
            turn_count: 0,
            event_sender,
        }
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

    /// Send user message and execute Turn.
    ///
    /// Returns a handle for receiving streaming events and the final result.
    pub async fn send_message(&mut self, user_input: String) -> TurnStreamHandle {
        // Compactor decides internally whether to compact
        // Clone the Arc first to avoid borrow conflicts
        let compactor = self.compactor.clone();
        if let Err(e) = compactor.compact(self).await {
            tracing::warn!("Compact failed: {}", e);
        }

        self.messages.push(ChatMessage::user(user_input));
        self.execute_turn_streaming().await
    }

    async fn execute_turn_streaming(&mut self) -> TurnStreamHandle {
        self.turn_count += 1;
        let turn_number = self.turn_count;
        let thread_id = self.id;

        // Create channels for streaming events and result
        let (stream_tx, mut stream_rx) = broadcast::channel(DEFAULT_CHANNEL_CAPACITY);
        let (llm_event_tx, llm_event_rx) = broadcast::channel(DEFAULT_CHANNEL_CAPACITY);
        let (result_tx, result_rx) = oneshot::channel();

        // Build TurnInput with stream_sender
        let tool_ids = self.tool_manager.list_ids();
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

        let turn_input = turn_input_builder.build();

        let event_sender = self.event_sender.clone();
        let config = self.config.turn_config.clone();

        // Spawn task to execute turn and forward events concurrently
        tokio::spawn(async move {
            // Clone event_sender for the forwarder task
            let forwarder_event_sender = event_sender.clone();

            // 1. Start event forwarding task (runs concurrently with turn execution)
            let forwarder = tokio::spawn(async move {
                while let Ok(event) = stream_rx.recv().await {
                    match event {
                        TurnStreamEvent::LlmEvent(llm_event) => {
                            // Send to ThreadEvent::Processing
                            let _ = forwarder_event_sender.send(ThreadEvent::Processing {
                                thread_id,
                                turn_number,
                                event: llm_event.clone(),
                            });
                            // Send to llm_events channel
                            let _ = llm_event_tx.send(llm_event);
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

            // 2. Execute turn (events are forwarded concurrently by the forwarder task)
            let result = execute_turn_streaming(turn_input, config).await;

            // 3. Wait for event forwarding to complete (channel closes after stream_tx is dropped)
            let _ = forwarder.await;

            // 4. Send final events based on result
            match &result {
                Ok(output) => {
                    let _ = event_sender.send(ThreadEvent::TurnCompleted {
                        thread_id,
                        turn_number,
                        token_usage: output.token_usage.clone(),
                    });
                }
                Err(e) => {
                    let _ = event_sender.send(ThreadEvent::TurnFailed {
                        thread_id,
                        turn_number,
                        error: e.to_string(),
                    });
                }
            }
            let _ = event_sender.send(ThreadEvent::Idle { thread_id });
            let _ = result_tx.send(result);
        });

        TurnStreamHandle {
            thread_id,
            turn_number,
            llm_events: llm_event_rx,
            result: result_rx,
        }
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
        // Use AssertUnwindSafe to allow catch_unwind with Arc<dyn Compactor>
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = ThreadBuilder::new().compactor(compactor).build();
        }));
        assert!(result.is_err());
    }

    #[test]
    fn thread_builder_requires_compactor() {
        let result = std::panic::catch_unwind(|| ThreadBuilder::new().build());
        assert!(result.is_err());
    }

    #[test]
    fn estimate_tokens_reasonable() {
        assert_eq!(Thread::estimate_tokens("test"), 1);
        assert_eq!(Thread::estimate_tokens("test test"), 2);
        assert_eq!(Thread::estimate_tokens(""), 1);
    }
}
