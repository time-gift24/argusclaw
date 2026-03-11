//! Thread implementation.

use std::sync::Arc;

use derive_builder::Builder;
use tokio::sync::{broadcast, oneshot};

use crate::agents::turn::{TurnError, TurnInputBuilder, TurnOutput, execute_turn};
use crate::approval::ApprovalManager;
use crate::llm::{ChatMessage, LlmProvider, LlmStreamEvent, Role};
use crate::tool::ToolManager;

use super::{CompactStrategy, ThreadConfig, ThreadError, ThreadEvent, ThreadId, ThreadState};

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

    /// Approval manager (optional).
    #[builder(default, setter(strip_option))]
    pub approval_manager: Option<Arc<ApprovalManager>>,

    /// Thread configuration.
    #[builder(default)]
    pub config: ThreadConfig,

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
    /// # Panics
    ///
    /// Panics if provider is not set.
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
            approval_manager: self.approval_manager.flatten(),
            config: self.config.unwrap_or_default(),
            token_count: 0,
            turn_count: 0,
            event_sender,
        }
    }
}

impl Thread {
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

    /// Check if compact is needed based on token threshold.
    pub fn should_compact(&self) -> bool {
        let context_window = 128_000u32;
        let threshold = (context_window as f32 * self.config.compact_threshold_ratio) as u32;
        self.token_count >= threshold
    }

    /// Send user message and execute Turn.
    ///
    /// Returns a handle for receiving streaming events and the final result.
    pub async fn send_message(&mut self, user_input: String) -> TurnStreamHandle {
        if self.should_compact() {
            let _ = self.compact().await;
        }

        self.messages.push(ChatMessage::user(user_input));
        self.execute_turn_streaming().await
    }

    async fn execute_turn_streaming(&mut self) -> TurnStreamHandle {
        self.turn_count += 1;
        let turn_number = self.turn_count;
        let thread_id = self.id;

        let turn_input = TurnInputBuilder::new()
            .provider(self.provider.clone())
            .messages(self.messages.clone())
            .tool_manager(self.tool_manager.clone())
            .tool_ids(self.tool_manager.list_ids())
            .build();

        let (_event_tx, event_rx) = broadcast::channel(DEFAULT_CHANNEL_CAPACITY);
        let (result_tx, result_rx) = oneshot::channel();

        let config = self.config.turn_config.clone();
        let event_sender = self.event_sender.clone();
        let messages = self.messages.clone();

        tokio::spawn(async move {
            let result = execute_turn(turn_input, config).await;

            match result {
                Ok(output) => {
                    let _ = event_sender.send(ThreadEvent::TurnCompleted {
                        thread_id,
                        turn_number,
                        token_usage: output.token_usage.clone(),
                    });
                    let _ = event_sender.send(ThreadEvent::Idle { thread_id });
                    let _ = result_tx.send(Ok(output));
                }
                Err(e) => {
                    let _ = event_sender.send(ThreadEvent::TurnFailed {
                        thread_id,
                        turn_number,
                        error: e.to_string(),
                    });
                    let _ = event_sender.send(ThreadEvent::Idle { thread_id });
                    let _ = result_tx.send(Err(e));
                }
            }
        });

        // Update local messages (simplified - in real impl, would wait for result)
        let _ = messages;

        TurnStreamHandle {
            thread_id,
            turn_number,
            llm_events: event_rx,
            result: result_rx,
        }
    }

    /// Compact the message history.
    pub async fn compact(&mut self) -> Result<(), ThreadError> {
        match &self.config.compact_strategy {
            CompactStrategy::KeepRecent { count } => {
                self.compact_keep_recent(*count);
            }
            CompactStrategy::KeepTokens { ratio } => {
                self.compact_keep_tokens(*ratio);
            }
            CompactStrategy::Summarize { .. } => {
                return Err(ThreadError::CompactFailed {
                    reason: "Summarize strategy not yet implemented".to_string(),
                });
            }
        }

        let _ = self.event_sender.send(ThreadEvent::Compacted {
            thread_id: self.id,
            new_token_count: self.token_count,
        });

        Ok(())
    }

    fn compact_keep_recent(&mut self, count: usize) {
        let system_msgs: Vec<_> = self
            .messages
            .iter()
            .filter(|m| m.role == Role::System)
            .cloned()
            .collect();

        let non_system: Vec<_> = self
            .messages
            .iter()
            .filter(|m| m.role != Role::System)
            .cloned()
            .collect();

        let start = non_system.len().saturating_sub(count);
        let recent: Vec<_> = non_system.into_iter().skip(start).collect();

        self.messages = [system_msgs, recent].concat();
        self.recalculate_token_count();
    }

    fn compact_keep_tokens(&mut self, ratio: f32) {
        let target_tokens = (self.token_count as f32 * ratio) as usize;
        self.truncate_to_token_budget(target_tokens);
    }

    fn truncate_to_token_budget(&mut self, target_tokens: usize) {
        let system_msgs: Vec<_> = self
            .messages
            .iter()
            .filter(|m| m.role == Role::System)
            .cloned()
            .collect();

        let mut kept: Vec<ChatMessage> = Vec::new();
        let mut current_tokens = 0u32;

        for msg in self.messages.iter().rev() {
            if msg.role == Role::System {
                continue;
            }
            let msg_tokens = Self::estimate_tokens(&msg.content);
            if current_tokens + msg_tokens > target_tokens as u32 {
                break;
            }
            kept.push(msg.clone());
            current_tokens += msg_tokens;
        }

        kept.reverse();
        self.messages = [system_msgs, kept].concat();
        self.token_count = current_tokens;
    }

    fn recalculate_token_count(&mut self) {
        self.token_count = self
            .messages
            .iter()
            .map(|m| Self::estimate_tokens(&m.content))
            .sum();
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

    #[test]
    fn thread_builder_requires_provider() {
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
