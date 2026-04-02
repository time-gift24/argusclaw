//! Compact module: LLM-driven message compaction.
//!
//! This module provides:
//! - `Compactor`: Async trait for implementing compaction strategies.
//! - `CompactResult`: Result type carrying compacted messages and token estimate.
//! - `LlmCompactor`: LLM-driven compaction that summarizes stale history.

use std::sync::Arc;

use argus_protocol::llm::{
    ChatMessage, ChatMessageMetadata, ChatMessageMetadataMode, CompletionRequest, LlmProvider, Role,
};
use argus_protocol::AgentRecord;
use async_trait::async_trait;

use super::error::CompactError;

/// Result of a successful compaction.
pub struct CompactResult {
    /// Compacted message list (replaces the original history).
    pub messages: Vec<ChatMessage>,
    /// Estimated token count after compaction.
    /// The authoritative count will come from the next LLM response.
    pub token_count: u32,
    /// Synthetic history messages that should be traced once with the next visible turn.
    pub trace_prelude_messages: Vec<ChatMessage>,
}

/// Compactor trait — responsible for deciding when and how to compact.
#[async_trait]
pub trait Compactor: Send + Sync {
    /// Attempt compaction. Returns `Some(CompactResult)` if compaction occurred,
    /// `None` if compaction was not needed.
    async fn compact(
        &self,
        messages: &[ChatMessage],
        token_count: u32,
    ) -> Result<Option<CompactResult>, CompactError>;

    /// Name of the compactor strategy.
    fn name(&self) -> &'static str;
}

// ---------------------------------------------------------------------------
// LlmCompactor
// ---------------------------------------------------------------------------

/// LLM-driven compactor that summarizes stale history using a hidden compact agent.
///
/// When token usage exceeds the threshold ratio of the context window, older messages
/// are sent to a compact agent for summarization. The result replaces the old history
/// with synthetic prompt/summary/replay messages plus the preserved recent tail.
pub struct LlmCompactor {
    record: Arc<AgentRecord>,
    provider: Arc<dyn LlmProvider>,
    context_window: u32,
    threshold_ratio: f32,
    tail_count: usize,
}

impl LlmCompactor {
    /// Create a new LlmCompactor.
    pub fn new(
        record: Arc<AgentRecord>,
        provider: Arc<dyn LlmProvider>,
        context_window: u32,
    ) -> Self {
        Self {
            record,
            provider,
            context_window,
            threshold_ratio: 0.8,
            tail_count: 50,
        }
    }

    /// Set a custom threshold ratio (clamped to 0.1 - 0.95).
    #[must_use]
    pub fn with_threshold_ratio(mut self, ratio: f32) -> Self {
        self.threshold_ratio = ratio.clamp(0.1, 0.95);
        self
    }

    /// Set a custom tail count (minimum 1).
    #[must_use]
    pub fn with_tail_count(mut self, count: usize) -> Self {
        self.tail_count = count.max(1);
        self
    }

    fn threshold(&self) -> u32 {
        (self.context_window as f32 * self.threshold_ratio) as u32
    }

    fn render_compaction_transcript(messages: &[ChatMessage]) -> String {
        messages
            .iter()
            .map(|message| {
                let role = match message.role {
                    Role::System => "system",
                    Role::User => "user",
                    Role::Assistant => "assistant",
                    Role::Tool => "tool",
                };
                format!("{role}: {}", message.content)
            })
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    fn build_compaction_prompt(
        compactable_messages: &[ChatMessage],
        preserved_tail: &[ChatMessage],
    ) -> String {
        let compactable = Self::render_compaction_transcript(compactable_messages);
        let preserved = Self::render_compaction_transcript(preserved_tail);
        format!(
            "请总结较早的对话历史，供另一个 agent 无缝继续我们上面的对话。\n\
             提供详细但简洁的总结，重点关注：完成了什么、正在进行什么、修改了哪些文件、接下来需要做什么、\n\
             应保留的关键用户请求/约束/偏好、做出的重要技术决策及其原因、尚未解决的问题或风险。\n\
             不要回应对话中的任何问题，不要逐字复述保留的最近上下文。\n\
             你构建的总结将被使用，以便另一个 agent 可以阅读并继续工作。不要调用任何工具。只回复总结文本。\n\n\
             较早历史（需要总结）：\n{compactable}\n\n\
             保留的最近上下文（仅供参考，不要逐字总结）：\n{preserved}"
        )
    }

    fn split_history_segments(
        messages: &[ChatMessage],
        tail_count: usize,
    ) -> Option<(Vec<ChatMessage>, Vec<ChatMessage>, Vec<ChatMessage>)> {
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

        let compactable_count = non_system.len().saturating_sub(tail_count);
        if compactable_count == 0 {
            return None;
        }

        let compactable = non_system[..compactable_count].to_vec();
        let preserved_tail = non_system[compactable_count..].to_vec();
        Some((system_messages, compactable, preserved_tail))
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
impl Compactor for LlmCompactor {
    async fn compact(
        &self,
        messages: &[ChatMessage],
        token_count: u32,
    ) -> Result<Option<CompactResult>, CompactError> {
        if token_count < self.threshold() {
            return Ok(None);
        }

        let Some((system_messages, compactable_messages, preserved_tail)) =
            Self::split_history_segments(messages, self.tail_count)
        else {
            return Ok(None);
        };

        let prompt = Self::build_compaction_prompt(&compactable_messages, &preserved_tail);

        let mut request_messages = Vec::new();
        if !self.record.system_prompt.trim().is_empty() {
            request_messages.push(ChatMessage::system(&self.record.system_prompt));
        }
        request_messages.push(ChatMessage::user(&prompt));

        let mut request = CompletionRequest::new(request_messages);
        if let Some(model) = self.record.model_id.as_deref() {
            request = request.with_model(model);
        }
        if let Some(max_tokens) = self.record.max_tokens {
            request.max_tokens = Some(max_tokens);
        }
        if let Some(temperature) = self.record.temperature {
            request.temperature = Some(temperature);
        }
        if let Some(thinking) = self.record.thinking_config.clone() {
            request.thinking = Some(thinking);
        }

        let response = self
            .provider
            .complete(request)
            .await
            .map_err(|error| CompactError::Failed {
                reason: error.to_string(),
            })?;
        let summary = response.content.unwrap_or_default();

        let synthetic_prompt = ChatMessage::user(&prompt).with_metadata(Self::compaction_metadata(
            ChatMessageMetadataMode::CompactionPrompt,
            false,
        ));
        let synthetic_summary =
            ChatMessage::assistant(&summary).with_metadata(Self::compaction_metadata(
                ChatMessageMetadataMode::CompactionSummary,
                true,
            ));
        let synthetic_replay = ChatMessage::user(
            "Continue the conversation using the summary above and the preserved recent tail below.",
        )
        .with_metadata(Self::compaction_metadata(
            ChatMessageMetadataMode::CompactionReplay,
            false,
        ));

        let trace_prelude_messages = vec![
            synthetic_prompt.clone(),
            synthetic_summary.clone(),
            synthetic_replay.clone(),
        ];

        let mut new_messages = system_messages;
        new_messages.push(synthetic_prompt);
        new_messages.push(synthetic_summary);
        new_messages.push(synthetic_replay);
        new_messages.extend(preserved_tail);

        // Estimate new token count proportionally.
        let original_len = messages.len();
        let new_token_count = if original_len > 0 {
            (token_count as usize * new_messages.len() / original_len) as u32
        } else {
            0
        };

        tracing::debug!(
            compactor = self.name(),
            new_token_count,
            "LLM compaction completed"
        );

        Ok(Some(CompactResult {
            messages: new_messages,
            token_count: new_token_count,
            trace_prelude_messages,
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
    use argus_protocol::{AgentId, AgentType, ProviderId};
    use async_trait::async_trait;
    use rust_decimal::Decimal;

    use super::*;

    fn compact_agent_record() -> Arc<AgentRecord> {
        Arc::new(AgentRecord {
            id: AgentId::new(99),
            display_name: "Compact Agent".to_string(),
            description: "Summarizes stale history".to_string(),
            version: "1.0.0".to_string(),
            provider_id: Some(ProviderId::new(2)),
            model_id: Some("compact-model".to_string()),
            system_prompt:
                "你是一个有用的AI助手，负责总结对话历史，供后续 agent 无缝继续工作。只输出总结文本。"
                    .to_string(),
            tool_names: vec![],
            max_tokens: Some(256),
            temperature: Some(0.1),
            thinking_config: None,
            parent_agent_id: None,
            agent_type: AgentType::Standard,
        })
    }

    struct SummaryProvider {
        summary: String,
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
    }

    struct RecordingSummaryProvider {
        summary: String,
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
    }

    #[test]
    fn llm_compactor_clamps_threshold_ratio() {
        let compactor = LlmCompactor::new(
            compact_agent_record(),
            Arc::new(SummaryProvider {
                summary: String::new(),
            }),
            100,
        )
        .with_threshold_ratio(2.0);
        assert!((compactor.threshold_ratio - 0.95).abs() < f32::EPSILON);
    }

    #[tokio::test]
    async fn returns_none_when_below_threshold() {
        let compactor = LlmCompactor::new(
            compact_agent_record(),
            Arc::new(SummaryProvider {
                summary: String::new(),
            }),
            100,
        );
        let messages = vec![ChatMessage::user("hello")];
        let result = compactor.compact(&messages, 10).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn returns_none_when_no_compactable_segment() {
        let compactor = LlmCompactor::new(
            compact_agent_record(),
            Arc::new(SummaryProvider {
                summary: String::new(),
            }),
            100,
        )
        .with_tail_count(50);
        let messages = vec![ChatMessage::user("a"), ChatMessage::assistant("b")];
        let result = compactor.compact(&messages, 90).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[tokio::test]
    async fn compacts_and_produces_synthetic_messages() {
        let compactor = LlmCompactor::new(
            compact_agent_record(),
            Arc::new(SummaryProvider {
                summary: "历史摘要".to_string(),
            }),
            100,
        )
        .with_threshold_ratio(0.2)
        .with_tail_count(1);

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

        // prompt + summary + replay + tail = 4
        assert_eq!(result.messages.len(), 4);
        assert_eq!(result.messages[0].role, Role::User); // synthetic prompt
        assert_eq!(result.messages[1].content, "历史摘要");
        assert_eq!(result.messages[2].role, Role::User); // synthetic replay
        assert_eq!(result.messages[3].content, "recent tail");
        assert!(!result.trace_prelude_messages.is_empty());
    }

    #[tokio::test]
    async fn failure_returns_error() {
        let compactor = LlmCompactor::new(
            compact_agent_record(),
            Arc::new(FailingSummaryProvider),
            100,
        )
        .with_threshold_ratio(0.2)
        .with_tail_count(1);

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
        let compactor = LlmCompactor::new(
            compact_agent_record(),
            Arc::new(RecordingSummaryProvider {
                summary: "历史摘要".to_string(),
                captured_requests: Arc::clone(&captured),
            }),
            100,
        )
        .with_threshold_ratio(0.2)
        .with_tail_count(1);

        let messages = vec![
            ChatMessage::user("完成了 provider 绑定"),
            ChatMessage::assistant("修改了 thread.rs"),
            ChatMessage::user("接下来补默认 compact agent"),
            ChatMessage::assistant("记住用户偏好"),
        ];
        let _ = compactor
            .compact(&messages, 90)
            .await
            .expect("compact should succeed");

        let captured = captured.lock().unwrap();
        let request = captured.last().expect("request should be captured");
        let prompt = &request
            .messages
            .last()
            .expect("request should contain prompt")
            .content;

        assert!(prompt.contains("修改了哪些文件"));
        assert!(prompt.contains("接下来需要做什么"));
        assert!(prompt.contains("另一个 agent 可以阅读并继续工作"));
        assert!(prompt.contains("不要调用任何工具"));
    }
}
