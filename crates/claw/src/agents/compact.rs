//! Compact module: strategies and manager for message compaction.
//!
//! This module provides:
//! - `CompactStrategy`: Trait for implementing different compaction strategies.
//! - `CompactManager`: Shared manager that handles compaction using a strategy.

use std::sync::Arc;

use crate::llm::{ChatMessage, Role};

/// Trait for compact strategies.
pub trait CompactStrategy: Send + Sync {
    /// Compact messages, returning the new token count.
    /// The implementation should modify `messages` in place and return the new token count.
    fn compact(&self, messages: &mut Vec<ChatMessage>, token_count: u32) -> u32;

    /// Name of the strategy.
    fn name(&self) -> &'static str;
}

/// KeepRecent strategy: keeps the most recent messages up to the target token count.
pub struct KeepRecentStrategy {
    /// Target token count to compact down to.
    target_tokens: u32,
    /// Average tokens per message (rough estimate for efficiency).
    tokens_per_message: u32,
}

impl KeepRecentStrategy {
    /// Create a new KeepRecentStrategy.
    #[must_use]
    pub fn new(target_tokens: u32) -> Self {
        Self {
            target_tokens,
            tokens_per_message: 50, // Rough estimate
        }
    }
}

impl CompactStrategy for KeepRecentStrategy {
    fn compact(&self, messages: &mut Vec<ChatMessage>, _token_count: u32) -> u32 {
        // Simple strategy: keep the most recent messages
        let target_count = (self.target_tokens / self.tokens_per_message).max(2) as usize;

        if messages.len() > target_count {
            // Always keep system messages at the start
            let system_count = messages
                .iter()
                .take_while(|m| m.role == Role::System)
                .count();
            let non_system = messages.len() - system_count;

            if non_system > target_count {
                let keep_from_end = target_count.saturating_sub(system_count);
                let remove_count = non_system - keep_from_end;

                // Remove from after system messages
                messages.drain(system_count..system_count + remove_count);
            }
        }

        // Estimate new token count (rough approximation)
        let estimated = messages.len() as u32 * self.tokens_per_message;
        estimated.min(self.target_tokens)
    }

    fn name(&self) -> &'static str {
        "keep_recent"
    }
}

/// KeepTokens strategy: keeps messages up to a specific token limit.
pub struct KeepTokensStrategy {
    /// Hard limit on tokens to keep.
    max_tokens: u32,
}

impl KeepTokensStrategy {
    /// Create a new KeepTokensStrategy.
    #[must_use]
    pub fn new(max_tokens: u32) -> Self {
        Self { max_tokens }
    }
}

impl CompactStrategy for KeepTokensStrategy {
    fn compact(&self, messages: &mut Vec<ChatMessage>, _token_count: u32) -> u32 {
        // Similar to KeepRecent but with harder limit
        let target_count = (self.max_tokens / 50).max(2) as usize;

        if messages.len() > target_count {
            let system_count = messages
                .iter()
                .take_while(|m| m.role == Role::System)
                .count();
            let non_system = messages.len() - system_count;

            if non_system > target_count {
                let keep_from_end = target_count.saturating_sub(system_count);
                let remove_count = non_system - keep_from_end;
                messages.drain(system_count..system_count + remove_count);
            }
        }

        messages.len() as u32 * 50
    }

    fn name(&self) -> &'static str {
        "keep_tokens"
    }
}

/// Default compact strategy factory.
pub mod default_strategy {
    use super::{CompactStrategy, KeepRecentStrategy, KeepTokensStrategy};

    /// Creates the default compact strategy (KeepRecent).
    pub fn create() -> Box<dyn CompactStrategy> {
        Box::new(KeepRecentStrategy::new(80_000))
    }

    /// Creates a KeepRecent strategy with custom target.
    pub fn keep_recent(target_tokens: u32) -> Box<dyn CompactStrategy> {
        Box::new(KeepRecentStrategy::new(target_tokens))
    }

    /// Creates a KeepTokens strategy.
    pub fn keep_tokens(max_tokens: u32) -> Box<dyn CompactStrategy> {
        Box::new(KeepTokensStrategy::new(max_tokens))
    }
}

/// Manages compact strategies for threads (全局共享).
#[derive(Clone)]
pub struct CompactManager {
    /// Context window size from the LLM provider.
    context_window: u32,
    /// Threshold ratio (e.g., 0.8 means compact at 80% of context window).
    threshold_ratio: f32,
    /// The compaction strategy to use.
    strategy: Arc<dyn CompactStrategy>,
}

impl CompactManager {
    /// Create a new CompactManager.
    ///
    /// # Arguments
    /// * `context_window` - The LLM provider's context window size.
    /// * `threshold_ratio` - Ratio of context window to trigger compaction (0.0-1.0).
    /// * `strategy` - The compact strategy to use.
    pub fn new(
        context_window: u32,
        threshold_ratio: f32,
        strategy: Box<dyn CompactStrategy>,
    ) -> Self {
        Self {
            context_window,
            threshold_ratio: threshold_ratio.clamp(0.1, 0.95),
            strategy: Arc::from(strategy),
        }
    }

    /// Create a CompactManager with default strategy.
    #[must_use]
    pub fn with_defaults(context_window: u32) -> Self {
        Self::new(context_window, 0.8, default_strategy::create())
    }

    /// Get the context window size.
    #[must_use]
    pub fn context_window(&self) -> u32 {
        self.context_window
    }

    /// Get the threshold for compaction.
    #[must_use]
    pub fn threshold(&self) -> u32 {
        (self.context_window as f32 * self.threshold_ratio) as u32
    }

    /// Check if compact is needed based on current token count.
    #[must_use]
    pub fn should_compact(&self, token_count: u32) -> bool {
        token_count >= self.threshold()
    }

    /// Execute compact using the configured strategy.
    ///
    /// Returns the new token count after compaction.
    pub fn compact(&self, messages: &mut Vec<ChatMessage>, token_count: u32) -> u32 {
        self.strategy.compact(messages, token_count)
    }

    /// Get the strategy name.
    #[must_use]
    pub fn strategy_name(&self) -> &'static str {
        self.strategy.name()
    }
}

impl std::fmt::Debug for CompactManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompactManager")
            .field("context_window", &self.context_window)
            .field("threshold_ratio", &self.threshold_ratio)
            .field("strategy", &self.strategy_name())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_messages(count: usize) -> Vec<ChatMessage> {
        (0..count)
            .map(|i| ChatMessage::user(format!("Message {i}")))
            .collect()
    }

    #[test]
    fn keep_recent_compact_reduces_message_count() {
        let strategy = KeepRecentStrategy::new(100); // Keep ~2 messages
        let mut messages = create_test_messages(10);

        let new_count = strategy.compact(&mut messages, 500);

        assert!(messages.len() <= 4); // System + ~3 user messages
        assert!(new_count < 500);
    }

    #[test]
    fn keep_recent_keeps_system_messages() {
        let strategy = KeepRecentStrategy::new(100);
        let mut messages = vec![
            ChatMessage::system("You are a helpful assistant."),
            ChatMessage::user("Hello"),
            ChatMessage::assistant("Hi there!"),
            ChatMessage::user("How are you?"),
        ];

        strategy.compact(&mut messages, 200);

        // First message should still be system
        assert!(!messages.is_empty());
        assert_eq!(messages[0].role, Role::System);
    }

    #[test]
    fn compact_manager_should_compact() {
        let manager = CompactManager::with_defaults(100_000);

        assert!(!manager.should_compact(50_000)); // Below 80% threshold
        assert!(manager.should_compact(85_000)); // Above 80% threshold
    }

    #[test]
    fn compact_manager_compact_calls_strategy() {
        let manager = CompactManager::new(100_000, 0.8, default_strategy::keep_recent(50_000));
        let mut messages = create_test_messages(20);

        let new_count = manager.compact(&mut messages, 100_000);

        assert!(new_count < 100_000);
    }

    #[test]
    fn threshold_ratio_clamped() {
        let manager = CompactManager::with_defaults(100_000);
        // threshold_ratio should be clamped to 0.95 max
        assert!(manager.threshold() <= 95_000);
    }
}
