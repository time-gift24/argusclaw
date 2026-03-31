//! Compact module: strategies and manager for message compaction.
//!
//! This module provides:
//! - `Compactor`: Async trait for implementing different compaction strategies.
//! - `KeepRecentCompactor`: Keeps the most recent messages up to a count.
//! - `KeepTokensCompactor`: Legacy compatibility strategy based on approximate token estimates.
//! - `CompactorManager`: Shared manager that handles compaction using a strategy.

use std::collections::HashMap;
use std::sync::Arc;

use argus_protocol::llm::{ChatMessage, LlmProvider, Role};
use async_trait::async_trait;

use super::error::CompactError;
use crate::tokenizer::{count_text_tokens, count_total_tokens};

/// Context for compaction operations.
///
/// This struct holds all the data needed for compaction without requiring
/// a reference to the full Thread type. Thread-level threshold overrides are
/// exposed here so built-in compactors can honor per-thread configuration
/// without changing the `Compactor` trait.
pub struct CompactContext<'a> {
    /// LLM provider for context window info.
    pub provider: &'a Arc<dyn LlmProvider>,
    /// Current token count.
    pub token_count: &'a mut u32,
    /// Messages to compact.
    pub messages: &'a mut Vec<ChatMessage>,
    /// Optional thread-level threshold ratio override for built-in compactors.
    threshold_ratio_override: Option<f32>,
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
            threshold_ratio_override: None,
        }
    }

    /// Apply a thread-level threshold ratio override for built-in compactors.
    #[must_use]
    pub fn with_threshold_ratio_override(mut self, threshold_ratio_override: f32) -> Self {
        self.threshold_ratio_override = Some(threshold_ratio_override.clamp(0.1, 0.95));
        self
    }

    /// Read the optional thread-level threshold override.
    #[must_use]
    pub fn threshold_ratio_override(&self) -> Option<f32> {
        self.threshold_ratio_override
    }

    /// Recalculate token count from the current messages using the compatibility tokenizer.
    pub fn recalculate_token_count(&mut self) -> Result<(), CompactError> {
        *self.token_count = count_total_tokens(self.messages.iter().map(|m| m.content.as_str()))?;
        Ok(())
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

    /// Number of recent non-system messages that should remain outside any synthetic summary.
    fn preserved_tail_count(&self) -> Option<usize> {
        None
    }
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
        let threshold_ratio = context
            .threshold_ratio_override()
            .unwrap_or(self.threshold_ratio);
        let threshold = (context_window as f32 * threshold_ratio) as u32;

        // Check if compaction is needed
        if *context.token_count < threshold {
            return Ok(());
        }

        // Perform compaction
        let messages = &mut *context.messages;
        let original_count = messages.len();

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

        // Update token count using proportional estimation.
        // The authoritative count will come from the next LLM response.
        let new_count = messages.len();
        if original_count > 0 {
            context.set_token_count(
                (*context.token_count as usize * new_count / original_count) as u32,
            );
        }

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

    fn preserved_tail_count(&self) -> Option<usize> {
        Some(self.keep_count)
    }
}

// ---------------------------------------------------------------------------
// KeepTokensCompactor
// ---------------------------------------------------------------------------

/// Legacy token-budget compactor kept for public API compatibility.
#[allow(deprecated)]
#[deprecated(note = "Prefer KeepRecentCompactor or LLM-driven compaction.")]
pub struct KeepTokensCompactor {
    /// Threshold ratio to trigger compaction (0.0 - 1.0).
    threshold_ratio: f32,
    /// Target ratio of context window to keep after compaction (0.0 - 1.0).
    target_ratio: f32,
}

#[allow(deprecated)]
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
#[allow(deprecated)]
impl Compactor for KeepTokensCompactor {
    async fn compact(&self, context: &mut CompactContext<'_>) -> Result<(), CompactError> {
        let context_window = context.provider.context_window();
        let threshold_ratio = context
            .threshold_ratio_override()
            .unwrap_or(self.threshold_ratio);
        let threshold = (context_window as f32 * threshold_ratio) as u32;

        if *context.token_count < threshold {
            return Ok(());
        }

        let target_tokens = (context_window as f32 * self.target_ratio) as u32;
        let messages = &mut *context.messages;

        let system_msgs: Vec<_> = messages
            .iter()
            .filter(|m| m.role == Role::System)
            .cloned()
            .collect();

        let mut kept: Vec<ChatMessage> = Vec::new();
        let mut current_tokens = 0u32;

        for msg in messages.iter().rev() {
            if msg.role == Role::System {
                continue;
            }
            let msg_tokens = count_text_tokens(&msg.content)?;
            if current_tokens + msg_tokens > target_tokens {
                break;
            }
            kept.push(msg.clone());
            current_tokens += msg_tokens;
        }

        kept.reverse();
        *messages = [system_msgs, kept].concat();
        context.recalculate_token_count()?;

        tracing::debug!(
            compactor = self.name(),
            new_token_count = *context.token_count,
            "Compatibility compaction completed"
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
    use std::sync::Arc;

    #[allow(deprecated)]
    use argus_protocol::LlmProvider;
    use argus_protocol::llm::{ChatMessage, CompletionRequest, CompletionResponse, LlmError};
    use async_trait::async_trait;
    use rust_decimal::Decimal;

    use super::*;

    struct FixedContextProvider {
        context_window: u32,
    }

    #[async_trait]
    impl LlmProvider for FixedContextProvider {
        fn model_name(&self) -> &str {
            "fixed-context"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            Err(LlmError::RequestFailed {
                provider: "fixed-context".to_string(),
                reason: "not implemented".to_string(),
            })
        }

        fn context_window(&self) -> u32 {
            self.context_window
        }
    }

    #[test]
    fn keep_recent_compactor_new_clamps_values() {
        let compactor = KeepRecentCompactor::new(2.0, 0);
        assert!((compactor.threshold_ratio - 0.95).abs() < f32::EPSILON);
        assert_eq!(compactor.keep_count, 1);
    }

    #[test]
    fn compactor_manager_defaults() {
        let manager = CompactorManager::with_defaults();
        assert_eq!(manager.default_compactor().name(), "keep_recent");
    }

    #[allow(deprecated)]
    #[test]
    fn keep_tokens_compactor_new_clamps_values() {
        let compactor = KeepTokensCompactor::new(2.0, 2.0);
        assert!((compactor.threshold_ratio - 0.95).abs() < f32::EPSILON);
        assert!((compactor.target_ratio - 0.9).abs() < f32::EPSILON);
    }

    #[allow(deprecated)]
    #[test]
    fn compactor_manager_register_and_get() {
        let mut manager = CompactorManager::with_defaults();
        manager.register("tokens", Arc::new(KeepTokensCompactor::with_defaults()));

        assert!(manager.get("tokens").is_some());
        assert_eq!(manager.get("tokens").unwrap().name(), "keep_tokens");
        assert!(manager.get("nonexistent").is_none());
    }

    #[tokio::test]
    async fn keep_recent_compactor_uses_context_threshold_override() {
        let provider: Arc<dyn LlmProvider> = Arc::new(FixedContextProvider {
            context_window: 100,
        });
        let repeated = ["test"; 10].join(" ");
        let mut messages = vec![
            ChatMessage::user(repeated.clone()),
            ChatMessage::user(repeated.clone()),
            ChatMessage::user(repeated.clone()),
        ];
        // Use a token count high enough to exceed the 20% threshold (20 tokens)
        let mut token_count = 90u32;
        let mut context = CompactContext::new(&provider, &mut token_count, &mut messages)
            .with_threshold_ratio_override(0.2);

        KeepRecentCompactor::new(0.8, 1)
            .compact(&mut context)
            .await
            .expect("override should force compaction");

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, repeated);
        // Proportional estimation: 90 * 1 / 3 = 30
        assert_eq!(token_count, 30);
    }
}
