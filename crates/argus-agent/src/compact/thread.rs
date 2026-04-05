//! Thread compaction: LLM-driven checkpoint summarization.
//!
//! This module provides:
//! - `ThreadCompactor`: Async trait for thread-level compaction strategies.
//! - `ThreadCompactResult`: Result type carrying checkpoint summary messages and token usage.
//! - `LlmThreadCompactor`: LLM-driven compaction that summarizes stale history.

use std::sync::Arc;

use argus_protocol::llm::{
    ChatMessage, ChatMessageMetadata, ChatMessageMetadataMode, CompletionRequest, LlmProvider, Role,
};
use argus_protocol::token_usage::TokenUsage;
use async_trait::async_trait;

use crate::error::CompactError;

const DEFAULT_COMPACTION_PROMPT: &str = "\
Provide a detailed prompt for continuing our conversation above.\n\
  Focus on information that would be helpful for continuing the conversation, including what we did, what we're doing, which files we're working on, and what we're going to do\n\
  next.\n\
  The summary that you construct will be used so that another agent can read it and continue the work.\n\
  Do not call any tools. Respond only with the summary text.\n\n\
  When constructing the summary, try to stick to this template:\n\
  ---\n\
  ## Goal\n\n\
  [What goal(s) is the user trying to accomplish?]\n\n\
  ## Instructions\n\n\
  - [What important instructions did the user give you that are relevant]\n\
  - [If there is a plan or spec, include information about it so next agent can continue using it]\n\n\
  ## Discoveries\n\n\
  [What notable things were learned during this conversation that would be useful for the next agent to know when continuing the work]\n\n\
  ## Accomplished\n\n\
  [What work has been completed, what work is still in progress, and what work is left]\n\n\
  ## Relevant files / directories\n\n\
  [Construct a structured list of relevant files that have been read, edited, or created that pertain to the task at hand. If all the files in a directory are relevant, include the\n\
   path to the directory.]\n\
  ---";

/// Result of a successful compaction.
#[derive(Debug, Clone)]
pub struct ThreadCompactResult {
    /// Summary messages to use in a compaction checkpoint.
    pub summary_messages: Vec<ChatMessage>,
    /// Authoritative token count for the compaction request + summary response.
    pub token_usage: TokenUsage,
}

/// Thread compactor trait — responsible for deciding when and how to compact.
#[async_trait]
pub trait ThreadCompactor: Send + Sync {
    /// Attempt compaction. Returns `Some(ThreadCompactResult)` if compaction occurred,
    /// `None` if compaction was not needed.
    async fn compact(
        &self,
        messages: &[ChatMessage],
        token_count: u32,
    ) -> Result<Option<ThreadCompactResult>, CompactError>;

    /// Name of the compactor strategy.
    fn name(&self) -> &'static str;
}

// ---------------------------------------------------------------------------
// LlmThreadCompactor
// ---------------------------------------------------------------------------

/// LLM-driven compactor that summarizes stale history using the current thread provider.
///
/// When token usage exceeds the threshold ratio of the context window, older messages
/// are sent to the current provider for summarization. The result replaces the old history
/// with checkpoint summary messages.
pub struct LlmThreadCompactor {
    provider: Arc<dyn LlmProvider>,
    threshold_ratio: f32,
}

impl LlmThreadCompactor {
    /// Create a new `LlmThreadCompactor`.
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            provider,
            threshold_ratio: 0.8,
        }
    }

    /// Set a custom threshold ratio (clamped to 0.1 - 0.95).
    #[must_use]
    pub fn with_threshold_ratio(mut self, ratio: f32) -> Self {
        self.threshold_ratio = ratio.clamp(0.1, 0.95);
        self
    }

    fn threshold(&self) -> u32 {
        (self.provider.context_window() as f32 * self.threshold_ratio) as u32
    }

    fn compaction_prompt() -> &'static str {
        DEFAULT_COMPACTION_PROMPT
    }

    fn build_compaction_request_messages(compactable_messages: &[ChatMessage]) -> Vec<ChatMessage> {
        let mut request_messages = Vec::with_capacity(compactable_messages.len() + 1);
        request_messages.extend(compactable_messages.iter().cloned());
        request_messages.push(ChatMessage::user(Self::compaction_prompt()));
        request_messages
    }

    fn split_history_segments(
        messages: &[ChatMessage],
    ) -> Option<(Vec<ChatMessage>, Vec<ChatMessage>)> {
        let system_messages: Vec<_> = messages
            .iter()
            .filter(|m| m.role == Role::System)
            .cloned()
            .collect();
        let non_system: Vec<_> = messages
            .iter()
            .filter(|m| m.role != Role::System)
            .cloned()
            .collect();

        if non_system.is_empty() {
            return None;
        }

        Some((system_messages, non_system))
    }

    fn compaction_metadata(mode: ChatMessageMetadataMode, summary: bool) -> ChatMessageMetadata {
        ChatMessageMetadata {
            summary,
            mode: Some(mode),
            synthetic: true,
            collapsed_by_default: true,
        }
    }
}

#[async_trait]
impl ThreadCompactor for LlmThreadCompactor {
    async fn compact(
        &self,
        messages: &[ChatMessage],
        token_count: u32,
    ) -> Result<Option<ThreadCompactResult>, CompactError> {
        if token_count < self.threshold() {
            return Ok(None);
        }

        let Some((system_messages, compactable_messages)) = Self::split_history_segments(messages)
        else {
            return Ok(None);
        };

        let request_messages = Self::build_compaction_request_messages(&compactable_messages);

        let request = CompletionRequest::new(request_messages);

        let response =
            self.provider
                .complete(request)
                .await
                .map_err(|error| CompactError::Failed {
                    reason: error.to_string(),
                })?;
        let summary = response.content.unwrap_or_default();

        let synthetic_summary = ChatMessage::assistant(&summary).with_metadata(
            Self::compaction_metadata(ChatMessageMetadataMode::CompactionSummary, true),
        );

        let mut summary_messages = system_messages.clone();
        summary_messages.push(synthetic_summary);

        tracing::debug!(compactor = self.name(), "LLM compaction completed");

        Ok(Some(ThreadCompactResult {
            summary_messages,
            token_usage: TokenUsage {
                input_tokens: response.input_tokens,
                output_tokens: response.output_tokens,
                total_tokens: response.input_tokens + response.output_tokens,
            },
        }))
    }

    fn name(&self) -> &'static str {
        "llm_compactor"
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use argus_protocol::llm::{CompletionRequest, CompletionResponse, LlmError};
    use async_trait::async_trait;
    use rust_decimal::Decimal;

    use super::*;

    struct SummaryProvider {
        summary: String,
        context_window: u32,
    }

    #[async_trait]
    impl LlmProvider for SummaryProvider {
        fn model_name(&self) -> &str {
            "summary-provider"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            Ok(CompletionResponse {
                content: Some(self.summary.clone()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: 12,
                output_tokens: 8,
                finish_reason: argus_protocol::llm::FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            })
        }

        fn context_window(&self) -> u32 {
            self.context_window
        }
    }

    struct FailingSummaryProvider;

    #[async_trait]
    impl LlmProvider for FailingSummaryProvider {
        fn model_name(&self) -> &str {
            "failing-summary-provider"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            Err(LlmError::RequestFailed {
                provider: "failing-summary-provider".to_string(),
                reason: "summary failed".to_string(),
            })
        }

        fn context_window(&self) -> u32 {
            100
        }
    }

    struct RecordingSummaryProvider {
        summary: String,
        context_window: u32,
        captured_requests: Arc<std::sync::Mutex<Vec<CompletionRequest>>>,
    }

    #[async_trait]
    impl LlmProvider for RecordingSummaryProvider {
        fn model_name(&self) -> &str {
            "recording-summary-provider"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            self.captured_requests.lock().unwrap().push(request);
            Ok(CompletionResponse {
                content: Some(self.summary.clone()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: 12,
                output_tokens: 8,
                finish_reason: argus_protocol::llm::FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            })
        }

        fn context_window(&self) -> u32 {
            self.context_window
        }
    }

    #[test]
    fn llm_compactor_clamps_threshold_ratio() {
        let provider = Arc::new(SummaryProvider {
            summary: String::new(),
            context_window: 100,
        });
        let compactor = LlmThreadCompactor::new(provider).with_threshold_ratio(2.0);
        assert!((compactor.threshold_ratio - 0.95).abs() < f32::EPSILON);
    }

    #[tokio::test]
    async fn returns_none_when_below_threshold() {
        let provider = Arc::new(SummaryProvider {
            summary: String::new(),
            context_window: 100,
        });
        let compactor = LlmThreadCompactor::new(provider);
        let messages = vec![ChatMessage::user("hello")];
        let result = compactor.compact(&messages, 10).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn returns_none_when_only_system_messages() {
        let provider = Arc::new(SummaryProvider {
            summary: String::new(),
            context_window: 100,
        });
        let compactor = LlmThreadCompactor::new(provider);
        let messages = vec![ChatMessage::system("system only")];
        let result = compactor.compact(&messages, 90).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn compacts_and_produces_synthetic_messages() {
        let provider = Arc::new(SummaryProvider {
            summary: "历史摘要".to_string(),
            context_window: 100,
        });
        let compactor = LlmThreadCompactor::new(provider).with_threshold_ratio(0.2);

        let messages = vec![
            ChatMessage::user("old question"),
            ChatMessage::assistant("old answer"),
            ChatMessage::user("recent tail"),
        ];
        let result = compactor
            .compact(&messages, 90)
            .await
            .expect("compact should succeed")
            .expect("should have compacted");

        assert_eq!(result.summary_messages.len(), 1);
        assert_eq!(result.summary_messages[0].role, Role::Assistant);
        assert_eq!(result.summary_messages[0].content, "历史摘要");
        assert_eq!(result.token_usage.total_tokens, 20);
    }

    #[tokio::test]
    async fn failure_returns_error() {
        let provider = Arc::new(FailingSummaryProvider);
        let compactor = LlmThreadCompactor::new(provider).with_threshold_ratio(0.2);

        let messages = vec![
            ChatMessage::user("old"),
            ChatMessage::assistant("reply"),
            ChatMessage::user("tail"),
        ];
        let result = compactor.compact(&messages, 90).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn prompt_includes_handoff_details() {
        let captured = Arc::new(std::sync::Mutex::new(Vec::new()));
        let provider = Arc::new(RecordingSummaryProvider {
            summary: "历史摘要".to_string(),
            context_window: 100,
            captured_requests: Arc::clone(&captured),
        });
        let compactor = LlmThreadCompactor::new(provider).with_threshold_ratio(0.2);

        let messages = vec![
            ChatMessage::user("完成了 provider 绑定"),
            ChatMessage::assistant("修改了 thread.rs"),
            ChatMessage::user("接下来补默认 compactor"),
            ChatMessage::assistant("记住用户偏好"),
        ];
        let _ = compactor
            .compact(&messages, 90)
            .await
            .expect("compact should succeed");

        let captured = captured.lock().unwrap();
        let request = captured.last().expect("request should be captured");
        let prompt_message = request
            .messages
            .last()
            .expect("request should contain prompt");
        assert_eq!(prompt_message.role, Role::User);
        assert!(request.model.is_none());
        assert!(request.temperature.is_none());
        let prompt = &prompt_message.content;

        assert!(
            prompt.contains("Provide a detailed prompt for continuing our conversation above.")
        );
        assert!(prompt.contains("Do not call any tools."));
        assert!(prompt.contains("## Goal"));
        assert!(prompt.contains("## Relevant files / directories"));
    }

    #[tokio::test]
    async fn compaction_request_preserves_history_and_appends_prompt() {
        let captured = Arc::new(std::sync::Mutex::new(Vec::new()));
        let provider = Arc::new(RecordingSummaryProvider {
            summary: "历史摘要".to_string(),
            context_window: 100,
            captured_requests: Arc::clone(&captured),
        });
        let compactor = LlmThreadCompactor::new(provider).with_threshold_ratio(0.2);

        let messages = vec![
            ChatMessage::system("system guardrails"),
            ChatMessage::user("old question"),
            ChatMessage::assistant("old answer"),
            ChatMessage::user("recent question"),
            ChatMessage::assistant("recent answer"),
        ];
        let _ = compactor
            .compact(&messages, 90)
            .await
            .expect("compact should succeed");

        let captured = captured.lock().unwrap();
        let request = captured.last().expect("request should be captured");
        assert_eq!(request.messages.len(), 5);
        assert_eq!(request.messages[0].content, "old question");
        assert_eq!(request.messages[1].content, "old answer");
        assert_eq!(request.messages[2].content, "recent question");
        assert_eq!(request.messages[3].content, "recent answer");
        assert_eq!(request.messages[4].role, Role::User);
        assert!(
            request.messages[4]
                .content
                .contains("Provide a detailed prompt for continuing our conversation above.")
        );
    }

    #[tokio::test]
    async fn uses_current_provider_context_window() {
        let messages = vec![
            ChatMessage::user("old question"),
            ChatMessage::assistant("old answer"),
            ChatMessage::user("recent tail"),
        ];
        let small_context_provider = Arc::new(SummaryProvider {
            summary: "small window summary".to_string(),
            context_window: 100,
        });
        let large_context_provider = Arc::new(SummaryProvider {
            summary: "large window summary".to_string(),
            context_window: 200,
        });
        let small_compactor =
            LlmThreadCompactor::new(small_context_provider).with_threshold_ratio(0.8);
        let large_compactor =
            LlmThreadCompactor::new(large_context_provider).with_threshold_ratio(0.8);

        let compacted = small_compactor
            .compact(&messages, 90)
            .await
            .expect("small window compact should succeed");
        assert!(compacted.is_some());

        let skipped = large_compactor
            .compact(&messages, 90)
            .await
            .expect("large window compact should succeed");
        assert!(skipped.is_none());
    }
}
