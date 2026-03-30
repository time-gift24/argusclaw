//! Compact module: strategies and manager for message compaction.
//!
//! This module provides:
//! - `Compactor`: Async trait for implementing different compaction strategies.
//! - `SummarizeCompactor`: LLM-based summarization (default).
//! - `KeepRecentCompactor`: Keeps the most recent messages up to a count.
//! - `KeepTokensCompactor`: Keeps messages within a token budget ratio.
//! - `CompactorManager`: Shared manager that handles compaction using a strategy.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use argus_protocol::llm::{ChatMessage, CompletionRequest, LlmProvider, Role};
use async_trait::async_trait;

use super::error::CompactError;
use crate::tokenizer::{count_total_tokens, estimate_tokens};

const SUMMARY_SYSTEM_PROMPT: &str = "\
You are a helpful AI assistant tasked with summarizing conversations. \
When asked to summarize, provide a detailed but concise summary that captures \
all essential context for continuing the conversation. Focus on: what was done, \
what is being worked on, which files are being modified, what needs to be done next, \
key user requests, constraints, preferences, and important technical decisions. \
Do not respond to any questions in the conversation, only output the summary.";

const SUMMARY_USER_PROMPT: &str = "\
Provide a detailed summary for continuing our conversation above.\n\
Focus on: what we did, what we're doing, which files we're working on, and what we're going to do next.\n\
The summary will be used so that another agent can read it and continue the work.\n\
Do not call any tools. Respond only with the summary text.\n\n\
---\n\
## Goal\n\n\
## Instructions\n\n\
## Discoveries\n\n\
## Accomplished\n\n\
## Relevant files / directories\n\
---";

const TOOL_OUTPUT_PLACEHOLDER: &str = "[Old tool result content cleared]";
const MAX_ASSISTANT_TEXT_CHARS: usize = 500;

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

    /// Recalculate token count from messages.
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
}

// ---------------------------------------------------------------------------
// SummarizeCompactor
// ---------------------------------------------------------------------------

/// SummarizeCompactor: uses LLM to summarize conversation history.
///
/// When token count exceeds `threshold_ratio` of the context window, this compactor
/// sends old messages to the LLM for summarization. The summary is inserted as a
/// system message, and recent messages are preserved verbatim.
///
/// On LLM failure or timeout, falls back to [`KeepRecentCompactor`].
pub struct SummarizeCompactor {
    /// Threshold ratio to trigger compaction (0.0 - 1.0).
    threshold_ratio: f32,
    /// Timeout in seconds for the summarization LLM call.
    summary_timeout_secs: u64,
    /// Number of recent non-system messages to preserve verbatim.
    keep_recent_count: usize,
    /// Fallback compactor when LLM summarization fails.
    fallback: KeepRecentCompactor,
}

impl SummarizeCompactor {
    /// Create a new SummarizeCompactor.
    #[must_use]
    pub fn new(threshold_ratio: f32) -> Self {
        Self {
            threshold_ratio: threshold_ratio.clamp(0.1, 0.95),
            summary_timeout_secs: 30,
            keep_recent_count: 6,
            fallback: KeepRecentCompactor::with_defaults(),
        }
    }

    /// Create with default settings (80% threshold).
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(0.8)
    }

    /// Set a custom timeout for the summarization LLM call.
    #[must_use]
    pub fn with_timeout(mut self, timeout_secs: u64) -> Self {
        self.summary_timeout_secs = timeout_secs.max(5);
        self
    }

    /// Set the number of recent non-system messages to preserve verbatim.
    #[must_use]
    pub fn with_keep_recent_count(mut self, count: usize) -> Self {
        self.keep_recent_count = count.max(2);
        self
    }
}

#[async_trait]
impl Compactor for SummarizeCompactor {
    async fn compact(&self, context: &mut CompactContext<'_>) -> Result<(), CompactError> {
        let context_window = context.provider.context_window();
        let threshold_ratio = context
            .threshold_ratio_override()
            .unwrap_or(self.threshold_ratio);
        let threshold = (context_window as f32 * threshold_ratio) as u32;

        if *context.token_count < threshold {
            return Ok(());
        }

        let messages = &mut *context.messages;

        // Split into system / old / recent
        let system_msgs: Vec<ChatMessage> = messages
            .iter()
            .filter(|m| m.role == Role::System)
            .cloned()
            .collect();

        let non_system: Vec<ChatMessage> = messages
            .iter()
            .filter(|m| m.role != Role::System)
            .cloned()
            .collect();

        let split_point = non_system.len().saturating_sub(self.keep_recent_count);
        if split_point == 0 {
            // Not enough old messages to summarize; nothing to do
            return Ok(());
        }

        let old_msgs: Vec<ChatMessage> = non_system[..split_point].to_vec();
        let recent_msgs: Vec<ChatMessage> = non_system[split_point..].to_vec();

        // Prune tool outputs in old messages to reduce token cost
        let mut pruned_old = old_msgs;
        prune_old_messages(&mut pruned_old);

        // Build summarization request
        let mut summary_messages: Vec<ChatMessage> =
            Vec::with_capacity(pruned_old.len() + 2);
        summary_messages.push(ChatMessage::system(SUMMARY_SYSTEM_PROMPT));
        summary_messages.extend(pruned_old);
        summary_messages.push(ChatMessage::user(SUMMARY_USER_PROMPT));

        let request = CompletionRequest::new(summary_messages)
            .with_max_tokens(4096)
            .with_temperature(0.3);

        // Call LLM with timeout
        let summary_result = tokio::time::timeout(
            Duration::from_secs(self.summary_timeout_secs),
            context.provider.complete(request),
        )
        .await;

        match summary_result {
            Ok(Ok(response)) => {
                let summary_text = match &response.content {
                    Some(text) if !text.is_empty() => text.clone(),
                    _ => {
                        tracing::warn!(
                            "Summarization returned empty content, falling back to keep_recent"
                        );
                        return self.fallback.compact(context).await;
                    }
                };

                // Reconstruct: system_msgs + summary + recent
                let mut new_messages =
                    Vec::with_capacity(system_msgs.len() + 1 + recent_msgs.len());
                new_messages.extend(system_msgs);
                new_messages.push(ChatMessage::system(format!(
                    "[Conversation Summary]\n{summary_text}"
                )));
                new_messages.extend(recent_msgs);

                *messages = new_messages;
                context.recalculate_token_count()?;

                tracing::debug!(
                    compactor = self.name(),
                    new_token_count = *context.token_count,
                    summary_len = summary_text.len(),
                    "Summarization compaction completed"
                );

                Ok(())
            }
            Ok(Err(e)) => {
                tracing::warn!(
                    error = %e,
                    "Summarization LLM call failed, falling back to keep_recent"
                );
                self.fallback.compact(context).await
            }
            Err(_) => {
                tracing::warn!(
                    timeout_secs = self.summary_timeout_secs,
                    "Summarization LLM call timed out, falling back to keep_recent"
                );
                self.fallback.compact(context).await
            }
        }
    }

    fn name(&self) -> &'static str {
        "summarize"
    }
}

/// Prune old messages to reduce token count before sending to the summarization LLM.
///
/// - Tool result messages have their content replaced with a placeholder.
/// - Assistant messages with tool calls have their text truncated.
fn prune_old_messages(messages: &mut [ChatMessage]) {
    for msg in messages.iter_mut() {
        match msg.role {
            Role::Tool => {
                msg.content = TOOL_OUTPUT_PLACEHOLDER.to_string();
            }
            Role::Assistant if msg.tool_calls.as_ref().is_some_and(|tc| !tc.is_empty()) => {
                if msg.content.len() > MAX_ASSISTANT_TEXT_CHARS {
                    msg.content.truncate(MAX_ASSISTANT_TEXT_CHARS);
                    msg.content.push_str("...");
                }
            }
            _ => {}
        }
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
        context.recalculate_token_count()?;

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
        let threshold_ratio = context
            .threshold_ratio_override()
            .unwrap_or(self.threshold_ratio);
        let threshold = (context_window as f32 * threshold_ratio) as u32;

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
            let msg_tokens = estimate_tokens(&msg.content)?;
            if current_tokens + msg_tokens > target_tokens as u32 {
                break;
            }
            kept.push(msg.clone());
            current_tokens += msg_tokens;
        }

        kept.reverse();
        *messages = [system_msgs, kept].concat();

        // Update token count
        context.recalculate_token_count()?;

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

    /// Create a CompactorManager with default SummarizeCompactor.
    #[must_use]
    pub fn with_defaults() -> Self {
        Self::new(Arc::new(SummarizeCompactor::with_defaults()))
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
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use argus_protocol::LlmProvider;
    use argus_protocol::llm::{
        ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LlmError,
    };
    use async_trait::async_trait;
    use rust_decimal::Decimal;
    use tokio::time::sleep;

    use super::*;

    // -- Mock providers --

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

    /// Mock provider that returns a configurable summary and captures the request.
    struct SummaryMockProvider {
        context_window: u32,
        response_content: String,
        should_fail: bool,
        delay: Option<Duration>,
        captured_messages: Mutex<Option<Vec<ChatMessage>>>,
    }

    impl SummaryMockProvider {
        fn new(context_window: u32, response_content: &str) -> Self {
            Self {
                context_window,
                response_content: response_content.to_string(),
                should_fail: false,
                delay: None,
                captured_messages: Mutex::new(None),
            }
        }

        fn failing(context_window: u32) -> Self {
            Self {
                context_window,
                response_content: String::new(),
                should_fail: true,
                delay: None,
                captured_messages: Mutex::new(None),
            }
        }

        fn with_delay(context_window: u32, delay: Duration) -> Self {
            Self {
                context_window,
                response_content: "summary".to_string(),
                should_fail: false,
                delay: Some(delay),
                captured_messages: Mutex::new(None),
            }
        }

    }

    #[async_trait]
    impl LlmProvider for SummaryMockProvider {
        fn model_name(&self) -> &str {
            "summary-mock"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            if let Some(delay) = self.delay {
                sleep(delay).await;
            }
            if self.should_fail {
                return Err(LlmError::RequestFailed {
                    provider: "summary-mock".to_string(),
                    reason: "mock failure".to_string(),
                });
            }
            *self.captured_messages.lock().unwrap() = Some(request.messages.clone());
            Ok(CompletionResponse {
                content: Some(self.response_content.clone()),
                reasoning_content: None,
                tool_calls: vec![],
                finish_reason: FinishReason::Stop,
                input_tokens: 0,
                output_tokens: 0,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            })
        }

        fn context_window(&self) -> u32 {
            self.context_window
        }
    }

    // -- KeepRecentCompactor tests --

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
        assert_eq!(manager.default_compactor().name(), "summarize");
    }

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
        let mut token_count = count_total_tokens(messages.iter().map(|m| m.content.as_str()))
            .expect("tokenization should succeed");
        let mut context = CompactContext::new(&provider, &mut token_count, &mut messages)
            .with_threshold_ratio_override(0.2);

        KeepRecentCompactor::new(0.8, 1)
            .compact(&mut context)
            .await
            .expect("override should force compaction");

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, repeated);
        assert_eq!(
            token_count,
            estimate_tokens(&messages[0].content).expect("tokenization should succeed")
        );
    }

    #[tokio::test]
    async fn keep_tokens_compactor_uses_context_threshold_override() {
        let provider: Arc<dyn LlmProvider> = Arc::new(FixedContextProvider {
            context_window: 100,
        });
        let repeated = ["test"; 10].join(" ");
        let mut messages = vec![
            ChatMessage::user(repeated.clone()),
            ChatMessage::user(repeated.clone()),
            ChatMessage::user(repeated.clone()),
        ];
        let mut token_count = count_total_tokens(messages.iter().map(|m| m.content.as_str()))
            .expect("tokenization should succeed");
        let mut context = CompactContext::new(&provider, &mut token_count, &mut messages)
            .with_threshold_ratio_override(0.2);

        KeepTokensCompactor::new(0.8, 0.2)
            .compact(&mut context)
            .await
            .expect("override should force compaction");

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].content, repeated);
        assert_eq!(messages[1].content, repeated);
        assert_eq!(
            token_count,
            count_total_tokens(messages.iter().map(|m| m.content.as_str()))
                .expect("tokenization should succeed")
        );
    }

    // -- SummarizeCompactor tests --

    #[test]
    fn summarize_compactor_new_clamps_values() {
        let compactor = SummarizeCompactor::new(2.0);
        assert!((compactor.threshold_ratio - 0.95).abs() < f32::EPSILON);
    }

    #[tokio::test]
    async fn summarize_compactor_no_op_below_threshold() {
        let provider: Arc<dyn LlmProvider> = Arc::new(SummaryMockProvider::new(1_000_000, "sum"));
        let mut messages = vec![
            ChatMessage::system("You are helpful"),
            ChatMessage::user("hello"),
        ];
        let mut token_count = 100u32;
        let mut context =
            CompactContext::new(&provider, &mut token_count, &mut messages);

        SummarizeCompactor::with_defaults()
            .compact(&mut context)
            .await
            .expect("no compaction needed");

        assert_eq!(messages.len(), 2);
        assert_eq!(messages[0].role, Role::System);
        assert_eq!(messages[1].role, Role::User);
    }

    #[tokio::test]
    async fn summarize_compactor_summarizes_on_threshold() {
        let provider: Arc<dyn LlmProvider> = Arc::new(SummaryMockProvider::new(
            100,
            "## Goal\nBuild a feature\n## Accomplished\nWrote tests",
        ));
        let mut messages = vec![
            ChatMessage::system("You are helpful"),
            ChatMessage::user("do task A"),
            ChatMessage::assistant("working on A"),
            ChatMessage::user("do task B"),
            ChatMessage::assistant("working on B"),
            ChatMessage::user("do task C"),
            ChatMessage::assistant("working on C"),
            ChatMessage::user("do task D"),
        ];
        let mut token_count = 200u32;
        let mut context =
            CompactContext::new(&provider, &mut token_count, &mut messages)
                .with_threshold_ratio_override(0.1);

        SummarizeCompactor::new(0.8)
            .with_keep_recent_count(3)
            .compact(&mut context)
            .await
            .expect("summarization should succeed");

        // Expected: system + summary_system + 3 recent messages = 5
        assert!(messages.len() >= 2);
        assert_eq!(messages[0].role, Role::System);
        assert!(messages[0].content.contains("helpful"));
        assert_eq!(messages[1].role, Role::System);
        assert!(messages[1].content.contains("Conversation Summary"));
        // Last 3 messages should be the recent ones
        assert_eq!(messages[messages.len() - 1].role, Role::User);
    }

    #[tokio::test]
    async fn summarize_compactor_falls_back_on_llm_error() {
        let provider: Arc<dyn LlmProvider> = Arc::new(SummaryMockProvider::failing(100));
        let repeated = ["test"; 10].join(" ");
        let mut messages = vec![
            ChatMessage::user(repeated.clone()),
            ChatMessage::user(repeated.clone()),
            ChatMessage::user(repeated.clone()),
        ];
        let mut token_count = 200u32;
        let mut context =
            CompactContext::new(&provider, &mut token_count, &mut messages)
                .with_threshold_ratio_override(0.1);

        SummarizeCompactor::with_defaults()
            .compact(&mut context)
            .await
            .expect("fallback should succeed");

        // Fallback KeepRecentCompactor keeps last 50 messages (all 3 fit)
        assert_eq!(messages.len(), 3);
        // No summary system message should be present
        assert!(messages.iter().all(|m| !m.content.contains("Conversation Summary")));
    }

    #[tokio::test]
    async fn summarize_compactor_falls_back_on_timeout() {
        let provider: Arc<dyn LlmProvider> =
            Arc::new(SummaryMockProvider::with_delay(100, Duration::from_secs(60)));
        let repeated = ["test"; 10].join(" ");
        let mut messages = vec![
            ChatMessage::user(repeated.clone()),
            ChatMessage::user(repeated.clone()),
            ChatMessage::user(repeated.clone()),
        ];
        let mut token_count = 200u32;
        let mut context =
            CompactContext::new(&provider, &mut token_count, &mut messages)
                .with_threshold_ratio_override(0.1);

        SummarizeCompactor::with_defaults()
            .with_timeout(1) // 1 second timeout
            .compact(&mut context)
            .await
            .expect("fallback should succeed on timeout");

        // Fallback keeps all 3 messages
        assert_eq!(messages.len(), 3);
    }

    #[tokio::test]
    async fn summarize_compactor_preserves_system_messages() {
        let provider: Arc<dyn LlmProvider> = Arc::new(SummaryMockProvider::new(
            100,
            "summary text",
        ));
        let mut messages = vec![
            ChatMessage::system("system prompt A"),
            ChatMessage::system("system prompt B"),
            ChatMessage::user("msg 1"),
            ChatMessage::assistant("resp 1"),
            ChatMessage::user("msg 2"),
            ChatMessage::assistant("resp 2"),
            ChatMessage::user("msg 3"),
            ChatMessage::assistant("resp 3"),
        ];
        let mut token_count = 200u32;
        let mut context =
            CompactContext::new(&provider, &mut token_count, &mut messages)
                .with_threshold_ratio_override(0.1);

        SummarizeCompactor::new(0.8)
            .with_keep_recent_count(2)
            .compact(&mut context)
            .await
            .expect("should succeed");

        // First two messages should be the original system prompts
        assert_eq!(messages[0].content, "system prompt A");
        assert_eq!(messages[1].content, "system prompt B");
        // Third message should be the summary
        assert!(messages[2].content.contains("Conversation Summary"));
    }

    #[tokio::test]
    async fn summarize_compactor_prunes_tool_outputs() {
        let provider: Arc<dyn LlmProvider> = Arc::new(SummaryMockProvider::new(
            100,
            "summary",
        ));
        let mut messages = vec![
            ChatMessage::system("system"),
            ChatMessage::user("read file A"),
            ChatMessage::assistant_with_tool_calls(
                Some("calling tool".to_string()),
                vec![argus_protocol::llm::ToolCall {
                    id: "tc1".to_string(),
                    name: "read_file".to_string(),
                    arguments: serde_json::Value::Null,
                }],
            ),
            ChatMessage::tool_result("tc1", "read_file", "very long file content here that would be expensive"),
            ChatMessage::user("now do B"),
            ChatMessage::assistant("done B"),
            ChatMessage::user("do C"),
        ];
        let mut token_count = 200u32;
        let mut context =
            CompactContext::new(&provider, &mut token_count, &mut messages)
                .with_threshold_ratio_override(0.1);

        SummarizeCompactor::new(0.8)
            .with_keep_recent_count(2)
            .compact(&mut context)
            .await
            .expect("should succeed");

        // The summary should NOT contain the original tool output text
        assert!(!messages.iter().any(|m| m.content.contains("very long file content")));
        // Recent messages should be preserved
        assert!(messages.iter().any(|m| m.content == "do C"));
    }
}
