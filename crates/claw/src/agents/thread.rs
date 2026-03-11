//! Thread module: represents a conversation thread with message history.
//!
//! A Thread manages the message history for a conversation and handles
//! compaction when the token count exceeds the context window threshold.

use std::fmt;
use std::sync::Arc;

use crate::agents::compact::CompactManager;
use crate::agents::turn::{execute_turn, TurnConfig, TurnInput, TurnInputBuilder, TurnOutput};
use crate::approval::ApprovalManager;
use crate::llm::{ChatMessage, LlmProvider};
use crate::tool::ToolManager;

/// Unique identifier for a thread.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ThreadId(String);

impl ThreadId {
    /// Creates a new thread ID.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl AsRef<str> for ThreadId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for ThreadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<String> for ThreadId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

/// Thread configuration.
#[derive(Debug, Clone)]
pub struct ThreadConfig {
    /// System prompt for the thread.
    pub system_prompt: String,
    /// Turn configuration.
    pub turn_config: TurnConfig,
}

impl ThreadConfig {
    /// Create a new ThreadConfig with default values.
    #[must_use]
    pub fn new() -> Self {
        Self {
            system_prompt: String::new(),
            turn_config: TurnConfig::new(),
        }
    }

    /// Create a ThreadConfig with a system prompt.
    #[must_use]
    pub fn with_system_prompt(system_prompt: impl Into<String>) -> Self {
        Self {
            system_prompt: system_prompt.into(),
            turn_config: TurnConfig::new(),
        }
    }
}

impl Default for ThreadConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Error type for thread operations.
#[derive(Debug, thiserror::Error)]
pub enum ThreadError {
    /// Thread not found.
    #[error("Thread not found: {0}")]
    NotFound(String),

    /// Turn execution failed.
    #[error("Turn execution failed: {0}")]
    TurnFailed(#[from] crate::agents::turn::TurnError),

    /// Compact failed.
    #[error("Compact failed: {0}")]
    CompactFailed(String),
}

/// Information about a thread.
#[derive(Debug, Clone)]
pub struct ThreadInfo {
    pub id: ThreadId,
    pub message_count: usize,
    pub token_count: u32,
}

impl ThreadInfo {
    #[must_use]
    pub fn new(id: ThreadId, message_count: usize, token_count: u32) -> Self {
        Self {
            id,
            message_count,
            token_count,
        }
    }
}

/// A conversation thread with message history and compaction support.
///
/// Thread manages the message history for a single conversation and uses
/// CompactManager to handle message compaction when approaching context limits.
pub struct Thread {
    /// Unique thread ID.
    id: ThreadId,
    /// Default LLM provider for this thread.
    provider: Arc<dyn LlmProvider>,
    /// Tool manager for this thread.
    tool_manager: Arc<ToolManager>,
    /// Compact manager for this thread.
    compact_manager: Arc<CompactManager>,
    /// Optional approval manager.
    approval_manager: Option<Arc<ApprovalManager>>,
    /// Thread configuration.
    config: ThreadConfig,
    /// Message history.
    messages: Vec<ChatMessage>,
    /// Estimated token count.
    token_count: u32,
}

impl Thread {
    /// Create a new Thread.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        provider: Arc<dyn LlmProvider>,
        tool_manager: Arc<ToolManager>,
        compact_manager: Arc<CompactManager>,
        approval_manager: Option<Arc<ApprovalManager>>,
        config: ThreadConfig,
    ) -> Self {
        let messages = if config.system_prompt.is_empty() {
            Vec::new()
        } else {
            vec![ChatMessage::system(&config.system_prompt)]
        };

        Self {
            id: ThreadId::new(uuid::Uuid::new_v4().to_string()),
            provider,
            tool_manager,
            compact_manager,
            approval_manager,
            config,
            messages,
            token_count: 0,
        }
    }

    /// Get the thread ID.
    #[must_use]
    pub fn id(&self) -> &ThreadId {
        &self.id
    }

    /// Get the message count.
    #[must_use]
    pub fn message_count(&self) -> usize {
        self.messages.len()
    }

    /// Get the estimated token count.
    #[must_use]
    pub fn token_count(&self) -> u32 {
        self.token_count
    }

    /// Get the messages.
    #[must_use]
    pub fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    /// Get the provider.
    #[must_use]
    pub fn provider(&self) -> &Arc<dyn LlmProvider> {
        &self.provider
    }

    /// Check if compact is needed.
    #[must_use]
    pub fn should_compact(&self) -> bool {
        self.compact_manager.should_compact(self.token_count)
    }

    /// Compact messages using the CompactManager.
    ///
    /// Returns the new token count after compaction.
    pub fn compact(&mut self) -> u32 {
        let new_count = self.compact_manager.compact(&mut self.messages, self.token_count);
        self.token_count = new_count;
        new_count
    }

    /// Add a user message to the thread.
    pub fn add_user_message(&mut self, content: impl Into<String>) {
        let message = ChatMessage::user(content);
        let token_count = self.estimate_token_count(&message);
        self.messages.push(message);
        self.token_count += token_count;
    }

    /// Add a message directly to the thread.
    pub fn add_message(&mut self, message: ChatMessage) {
        let token_count = self.estimate_token_count(&message);
        self.messages.push(message);
        self.token_count += token_count;
    }

    /// Execute a turn with the current message history.
    ///
    /// This adds the user message, executes a turn, and returns the output.
    pub async fn execute_turn(&mut self, user_message: impl Into<String>) -> Result<TurnOutput, ThreadError> {
        // Add user message
        self.add_user_message(user_message);

        // Check if compact is needed before execution
        if self.should_compact() {
            self.compact();
        }

        // Build turn input
        let input = TurnInputBuilder::new()
            .messages(self.messages.clone())
            .system_prompt(self.config.system_prompt.clone())
            .provider(self.provider.clone())
            .tool_manager(self.tool_manager.clone())
            .build();

        // Execute turn
        let output = execute_turn(input, self.config.turn_config.clone()).await?;

        // Update messages and token count
        self.messages = output.messages.clone();
        self.token_count += output.token_usage.total_tokens;

        // Check if compact is needed after execution
        if self.should_compact() {
            self.compact();
        }

        Ok(output)
    }

    /// Execute a turn with a pre-built TurnInput.
    pub async fn execute(&mut self, input: TurnInput) -> Result<TurnOutput, ThreadError> {
        // Check if compact is needed before execution
        if self.should_compact() {
            self.compact();
        }

        // Use default turn config since caller provides TurnInput
        let output = execute_turn(input, TurnConfig::new()).await?;

        // Update messages and token count
        self.messages = output.messages.clone();
        self.token_count += output.token_usage.total_tokens;

        // Check if compact is needed after execution
        if self.should_compact() {
            self.compact();
        }

        Ok(output)
    }

    /// Get thread info.
    #[must_use]
    pub fn info(&self) -> ThreadInfo {
        ThreadInfo::new(
            self.id.clone(),
            self.messages.len(),
            self.token_count,
        )
    }

    /// Estimate token count for a message (rough approximation).
    fn estimate_token_count(&self, message: &ChatMessage) -> u32 {
        // Rough estimate: ~4 characters per token
        let content_tokens = message.content.len() as u32 / 4;
        // Add overhead for role and other metadata
        content_tokens + 10
    }

    /// Update the provider for this thread.
    pub fn set_provider(&mut self, provider: Arc<dyn LlmProvider>) {
        self.provider = provider;
    }
}

impl fmt::Debug for Thread {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Thread")
            .field("id", &self.id)
            .field("message_count", &self.messages.len())
            .field("token_count", &self.token_count)
            .field("config", &self.config)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_thread_id_display() {
        let id = ThreadId::new("test-thread");
        assert_eq!(id.to_string(), "test-thread");
    }

    #[test]
    fn test_thread_config_defaults() {
        let config = ThreadConfig::new();
        assert!(config.system_prompt.is_empty());
    }

    #[test]
    fn test_thread_config_with_system_prompt() {
        let config = ThreadConfig::with_system_prompt("You are helpful.");
        assert_eq!(config.system_prompt, "You are helpful.");
    }

    #[test]
    fn test_thread_info() {
        let info = ThreadInfo::new(ThreadId::new("test"), 10, 500);
        assert_eq!(info.message_count, 10);
        assert_eq!(info.token_count, 500);
    }
}
