//! Compact module: strategies and manager for message compaction.
//!
//! This module provides:
//! - `Compactor`: Async trait for implementing different compaction strategies.
//! - `KeepRecentCompactor`: Keeps the most recent messages up to a count.
//! - `KeepTokensCompactor`: Keeps messages within a token budget ratio.
//! - `CompactorManager`: Shared manager that handles compaction using a strategy.

use std::collections::HashMap;
use std::sync::Arc;

use argus_protocol::llm::{ChatMessage, LlmProvider, Role};
use argus_protocol::estimate_message_tokens;
use async_trait::async_trait;

use super::error::CompactError;

/// Context for compaction operations.
///
/// This struct holds all the data needed for compaction without requiring
/// a reference to the full Thread type.
pub struct CompactContext<'a> {
    /// LLM provider for context window info.
    pub provider: &'a Arc<dyn LlmProvider>,
    /// Current token count.
    pub token_count: &'a mut u32,
    /// Messages to compact.
    pub messages: &'a mut Vec<ChatMessage>,
}

impl<'a> CompactContext<'a> {
    /// Create a new CompactContext.
    pub fn new(
        provider: &'a Arc<dyn LlmProvider>,
        token_count: &'a mut u32,
        messages: &'a mut Vec<ChatMessage>,
    ) -> Self {
        Self {
            provider,
            token_count,
            messages,
        }
    }

    /// Recalculate token count from messages.
    pub fn recalculate_token_count(&mut self) {
        use argus_protocol::estimate_message_tokens;
        *self.token_count = self
            .messages
            .iter()
            .map(|m| estimate_message_tokens(m) as u32)
            .sum();
    }

    /// Set the token count.
    pub fn set_token_count(&mut self, count: u32) {
        *self.token_count = count;
    }
}

/// Compactor trait - responsible for deciding when and how to compact.
///
/// Implementations determine:
/// 1. Whether compaction is needed based on context state
/// 2. How to perform the compaction when needed
#[async_trait]
pub trait Compactor: Send + Sync {
    /// Check if compaction is needed and perform it if so.
    ///
    /// Returns `Ok(())` on success (may be a no-op if compaction wasn't needed).
    /// Returns an error if compaction was needed but failed.
    async fn compact(&self, context: &mut CompactContext<'_>) -> Result<(), CompactError>;

    /// Name of the compactor strategy.
    fn name(&self) -> &'static str;
}

// ---------------------------------------------------------------------------
// KeepRecentCompactor
// ---------------------------------------------------------------------------

/// KeepRecentCompactor: keeps the most recent N messages.
///
/// Compaction triggers when token count exceeds `threshold_ratio` of context window.
/// When triggered, keeps system messages + the most recent `keep_count` non-system messages.
pub struct KeepRecentCompactor {
    /// Threshold ratio to trigger compaction (0.0 - 1.0).
    threshold_ratio: f32,
    /// Number of recent non-system messages to keep.
    keep_count: usize,
}

impl KeepRecentCompactor {
    /// Create a new KeepRecentCompactor.
    #[must_use]
    pub fn new(threshold_ratio: f32, keep_count: usize) -> Self {
        Self {
            threshold_ratio: threshold_ratio.clamp(0.1, 0.95),
            keep_count: keep_count.max(1),
        }
    }

    /// Create with default settings (80% threshold, keep 50 messages).
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(0.8, 50)
    }
}

#[async_trait]
impl Compactor for KeepRecentCompactor {
    async fn compact(&self, context: &mut CompactContext<'_>) -> Result<(), CompactError> {
        let context_window = context.provider.context_window();
        let threshold = (context_window as f32 * self.threshold_ratio) as u32;

        // Check if compaction is needed
        if *context.token_count < threshold {
            return Ok(());
        }

        // Perform compaction
        let messages = &mut *context.messages;

        // Extract system messages
        let system_msgs: Vec<_> = messages
            .iter()
            .filter(|m| m.role == Role::System)
            .cloned()
            .collect();

        // Extract non-system messages
        let non_system: Vec<_> = messages
            .iter()
            .filter(|m| m.role != Role::System)
            .cloned()
            .collect();

        // Keep the most recent N non-system messages
        let start = non_system.len().saturating_sub(self.keep_count);
        let recent: Vec<_> = non_system.into_iter().skip(start).collect();

        // Reconstruct messages
        *messages = [system_msgs, recent].concat();

        // Update token count
        context.recalculate_token_count();

        tracing::debug!(
            compactor = self.name(),
            new_token_count = *context.token_count,
            "Compaction completed"
        );

        Ok(())
    }

    fn name(&self) -> &'static str {
        "keep_recent"
    }
}

// ---------------------------------------------------------------------------
// KeepTokensCompactor
// ---------------------------------------------------------------------------

/// KeepTokensCompactor: keeps messages within a token budget.
///
/// Compaction triggers when token count exceeds `threshold_ratio` of context window.
/// When triggered, keeps system messages + messages totaling up to `target_ratio` of context window.
pub struct KeepTokensCompactor {
    /// Threshold ratio to trigger compaction (0.0 - 1.0).
    threshold_ratio: f32,
    /// Target ratio of context window to keep after compaction (0.0 - 1.0).
    target_ratio: f32,
}

impl KeepTokensCompactor {
    /// Create a new KeepTokensCompactor.
    #[must_use]
    pub fn new(threshold_ratio: f32, target_ratio: f32) -> Self {
        Self {
            threshold_ratio: threshold_ratio.clamp(0.1, 0.95),
            target_ratio: target_ratio.clamp(0.1, 0.9),
        }
    }

    /// Create with default settings (80% threshold, keep 50% of context).
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(0.8, 0.5)
    }
}

#[async_trait]
impl Compactor for KeepTokensCompactor {
    async fn compact(&self, context: &mut CompactContext<'_>) -> Result<(), CompactError> {
        let context_window = context.provider.context_window();
        let threshold = (context_window as f32 * self.threshold_ratio) as u32;

        // Check if compaction is needed
        if *context.token_count < threshold {
            return Ok(());
        }

        // Calculate target token budget
        let target_tokens = (context_window as f32 * self.target_ratio) as usize;

        // Perform compaction
        let messages = &mut *context.messages;

        // Extract system messages
        let system_msgs: Vec<_> = messages
            .iter()
            .filter(|m| m.role == Role::System)
            .cloned()
            .collect();

        // Build list from end, respecting token budget
        let mut kept: Vec<ChatMessage> = Vec::new();
        let mut current_tokens = 0u32;

        for msg in messages.iter().rev() {
            if msg.role == Role::System {
                continue;
            }
            let msg_tokens = estimate_message_tokens(msg) as u32;
            if current_tokens + msg_tokens > target_tokens as u32 {
                break;
            }
            kept.push(msg.clone());
            current_tokens += msg_tokens;
        }

        kept.reverse();
        *messages = [system_msgs, kept].concat();

        // Update token count
        context.set_token_count(current_tokens);

        tracing::debug!(
            compactor = self.name(),
            new_token_count = *context.token_count,
            "Compaction completed"
        );

        Ok(())
    }

    fn name(&self) -> &'static str {
        "keep_tokens"
    }
}

// ---------------------------------------------------------------------------
// CompactorManager
// ---------------------------------------------------------------------------

/// Manages Compactor instances for agents.
///
/// Provides a default compactor and allows registration of named compactors.
#[derive(Clone)]
pub struct CompactorManager {
    /// Default compactor.
    default: Arc<dyn Compactor>,
    /// Registered compactors by name.
    compactors: HashMap<String, Arc<dyn Compactor>>,
}

impl CompactorManager {
    /// Create a new CompactorManager with a default compactor.
    #[must_use]
    pub fn new(default: Arc<dyn Compactor>) -> Self {
        Self {
            default,
            compactors: HashMap::new(),
        }
    }

    /// Create a CompactorManager with default KeepRecentCompactor.
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(Arc::new(KeepRecentCompactor::with_defaults()))
    }

    /// Get the default compactor.
    #[must_use]
    pub fn default_compactor(&self) -> &Arc<dyn Compactor> {
        &self.default
    }

    /// Register a named compactor.
    pub fn register(&mut self, name: &str, compactor: Arc<dyn Compactor>) {
        self.compactors.insert(name.to_string(), compactor);
    }

    /// Get a compactor by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Arc<dyn Compactor>> {
        self.compactors.get(name)
    }
}

impl std::fmt::Debug for CompactorManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CompactorManager")
            .field("default", &self.default.name())
            .field("registered", &self.compactors.keys().collect::<Vec<_>>())
            .finish()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keep_recent_compactor_new_clamps_values() {
        let compactor = KeepRecentCompactor::new(2.0, 0);
        assert!((compactor.threshold_ratio - 0.95).abs() < f32::EPSILON);
        assert_eq!(compactor.keep_count, 1);
    }

    #[test]
    fn keep_tokens_compactor_new_clamps_values() {
        let compactor = KeepTokensCompactor::new(2.0, 2.0);
        assert!((compactor.threshold_ratio - 0.95).abs() < f32::EPSILON);
        assert!((compactor.target_ratio - 0.9).abs() < f32::EPSILON);
    }

    #[test]
    fn compactor_manager_defaults() {
        let manager = CompactorManager::with_defaults();
        assert_eq!(manager.default_compactor().name(), "keep_recent");
    }

    #[test]
    fn compactor_manager_register_and_get() {
        let mut manager = CompactorManager::with_defaults();
        manager.register("tokens", Arc::new(KeepTokensCompactor::with_defaults()));

        assert!(manager.get("tokens").is_some());
        assert_eq!(manager.get("tokens").unwrap().name(), "keep_tokens");
        assert!(manager.get("nonexistent").is_none());
    }
}
