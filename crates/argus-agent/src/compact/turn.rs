use std::sync::Arc;

use argus_protocol::llm::{
    ChatMessage, ChatMessageMetadata, ChatMessageMetadataMode, CompletionRequest, LlmProvider, Role,
};

use super::{CompactResult, Compactor};
use crate::error::CompactError;

const USER_HISTORY_TOKEN_BUDGET: usize = 20_000;
const DEFAULT_THRESHOLD_RATIO: f32 = 0.8;
const TURN_COMPACTION_PROMPT: &str = "\
Write a compact continuation message from the user's perspective.\n\
Summarize what I asked you to do, what you already discovered or completed,\n\
what context still matters, and what I want you to continue doing next.\n\
Write the summary as if it were written by the user in first person.\n\
Do not call any tools. Do not write from the assistant perspective.\n\
Respond only with the summary text.";

pub struct LlmTurnCompactor {
    provider: Arc<dyn LlmProvider>,
    threshold_ratio: f32,
}

impl LlmTurnCompactor {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self {
            provider,
            threshold_ratio: DEFAULT_THRESHOLD_RATIO,
        }
    }

    #[must_use]
    pub fn with_threshold_ratio(mut self, ratio: f32) -> Self {
        self.threshold_ratio = ratio.clamp(0.1, 0.95);
        self
    }

    fn estimated_tokens(content: &str) -> usize {
        (content.len().saturating_add(3)) / 4
    }

    fn threshold(&self) -> u32 {
        (self.provider.context_window() as f32 * self.threshold_ratio) as u32
    }

    fn split_message_segments(
        messages: &[ChatMessage],
    ) -> (Vec<ChatMessage>, Vec<ChatMessage>, Vec<ChatMessage>) {
        let system_messages = messages
            .iter()
            .filter(|message| message.role == Role::System)
            .cloned()
            .collect();
        let user_messages = messages
            .iter()
            .filter(|message| message.role == Role::User)
            .cloned()
            .collect();
        let non_user_messages = messages
            .iter()
            .filter(|message| !matches!(message.role, Role::System | Role::User))
            .cloned()
            .collect();

        (system_messages, user_messages, non_user_messages)
    }

    fn select_recent_user_inputs(messages: &[ChatMessage]) -> Vec<ChatMessage> {
        let (_, user_inputs, _) = Self::split_message_segments(messages);

        let mut selected = Vec::new();
        let mut used_budget = 0usize;
        for message in user_inputs.into_iter().rev() {
            let estimate = Self::estimated_tokens(&message.content);
            if !selected.is_empty()
                && used_budget.saturating_add(estimate) > USER_HISTORY_TOKEN_BUDGET
            {
                break;
            }
            used_budget = used_budget.saturating_add(estimate);
            selected.push(message);
        }

        selected
    }

    fn summary_metadata() -> ChatMessageMetadata {
        ChatMessageMetadata {
            summary: true,
            mode: Some(ChatMessageMetadataMode::CompactionSummary),
            synthetic: true,
            collapsed_by_default: true,
        }
    }

    fn build_request_messages(
        system_messages: &[ChatMessage],
        selected_history: &[ChatMessage],
        non_user_messages: &[ChatMessage],
    ) -> Vec<ChatMessage> {
        let mut request_messages = Vec::with_capacity(
            system_messages
                .len()
                .saturating_add(selected_history.len())
                .saturating_add(non_user_messages.len())
                .saturating_add(1),
        );

        request_messages.extend(system_messages.iter().cloned());
        request_messages.extend(selected_history.iter().cloned());
        request_messages.extend(non_user_messages.iter().cloned());
        request_messages.push(ChatMessage::user(TURN_COMPACTION_PROMPT).with_metadata(
            ChatMessageMetadata {
                summary: false,
                mode: Some(ChatMessageMetadataMode::CompactionPrompt),
                synthetic: true,
                collapsed_by_default: true,
            },
        ));

        request_messages
    }
}

#[async_trait::async_trait]
impl Compactor for LlmTurnCompactor {
    async fn compact(
        &self,
        messages: &[ChatMessage],
        token_count: u32,
    ) -> Result<Option<CompactResult>, CompactError> {
        if token_count < self.threshold() {
            return Ok(None);
        }

        let (system_messages, _, non_user_messages) = Self::split_message_segments(messages);
        let selected_history = Self::select_recent_user_inputs(messages);
        if selected_history.is_empty() && non_user_messages.is_empty() {
            return Ok(None);
        }

        let request = CompletionRequest::new(Self::build_request_messages(
            &system_messages,
            &selected_history,
            &non_user_messages,
        ));
        let response =
            self.provider
                .complete(request)
                .await
                .map_err(|error| CompactError::Failed {
                    reason: error.to_string(),
                })?;
        let summary = response.content.unwrap_or_default();
        if summary.trim().is_empty() {
            return Ok(None);
        }

        let mut checkpoint_messages = selected_history;
        checkpoint_messages
            .push(ChatMessage::user(summary).with_metadata(Self::summary_metadata()));
        Ok(Some(CompactResult {
            messages: checkpoint_messages,
            token_usage: argus_protocol::TokenUsage {
                input_tokens: response.input_tokens,
                output_tokens: response.output_tokens,
                total_tokens: response.input_tokens + response.output_tokens,
            },
        }))
    }

    fn name(&self) -> &'static str {
        "llm_turn_compactor"
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use argus_protocol::llm::{CompletionResponse, LlmError};
    use async_trait::async_trait;
    use rust_decimal::Decimal;

    use super::*;

    #[derive(Debug)]
    struct CapturingSummaryProvider {
        requests: Arc<Mutex<Vec<CompletionRequest>>>,
        summary: String,
        context_window: u32,
    }

    impl CapturingSummaryProvider {
        fn new(
            requests: Arc<Mutex<Vec<CompletionRequest>>>,
            summary: impl Into<String>,
            context_window: u32,
        ) -> Self {
            Self {
                requests,
                summary: summary.into(),
                context_window,
            }
        }
    }

    #[async_trait]
    impl LlmProvider for CapturingSummaryProvider {
        fn model_name(&self) -> &str {
            "capturing-turn-summary"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            self.requests.lock().unwrap().push(request);
            Ok(CompletionResponse {
                content: Some(self.summary.clone()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: 4,
                output_tokens: 2,
                finish_reason: argus_protocol::llm::FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            })
        }

        fn context_window(&self) -> u32 {
            self.context_window
        }
    }

    #[tokio::test]
    async fn compact_keeps_recent_user_inputs_in_reverse_chronological_order() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let compactor = LlmTurnCompactor::new(Arc::new(CapturingSummaryProvider::new(
            Arc::clone(&requests),
            "continue from here",
            100,
        )))
        .with_threshold_ratio(0.2);

        let result = compactor
            .compact(
                &[
                    ChatMessage::system("system prompt"),
                    ChatMessage::user("oldest user"),
                    ChatMessage::assistant("oldest assistant"),
                    ChatMessage::user("newer user"),
                    ChatMessage::assistant("tool context"),
                    ChatMessage::user("latest user"),
                ],
                90,
            )
            .await
            .expect("turn compact should succeed")
            .expect("turn compact should produce a result");

        let checkpoint_contents: Vec<_> = result
            .messages
            .iter()
            .map(|message| message.content.as_str())
            .collect();
        assert_eq!(
            checkpoint_contents,
            vec![
                "latest user",
                "newer user",
                "oldest user",
                "continue from here"
            ]
        );
    }

    #[tokio::test]
    async fn compact_includes_system_prompt_in_request_but_not_compacted_messages() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let compactor = LlmTurnCompactor::new(Arc::new(CapturingSummaryProvider::new(
            Arc::clone(&requests),
            "please continue the task",
            100,
        )))
        .with_threshold_ratio(0.2);

        let result = compactor
            .compact(
                &[
                    ChatMessage::system("You are a helpful assistant."),
                    ChatMessage::user("older request"),
                    ChatMessage::assistant("recent tool output"),
                ],
                90,
            )
            .await
            .expect("turn compact should succeed")
            .expect("turn compact should produce a result");

        let captured = requests.lock().unwrap();
        let request = captured.first().expect("provider should capture a request");
        assert_eq!(request.messages[0].role, Role::System);
        assert_eq!(request.messages[0].content, "You are a helpful assistant.");
        assert!(
            result
                .messages
                .iter()
                .all(|message| message.role != Role::System)
        );
        assert_eq!(
            result
                .messages
                .last()
                .expect("summary message should exist")
                .role,
            Role::User
        );
    }

    #[tokio::test]
    async fn compact_uses_internal_threshold_instead_of_always_compacting() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let compactor = LlmTurnCompactor::new(Arc::new(CapturingSummaryProvider::new(
            Arc::clone(&requests),
            "summary",
            100,
        )));

        let result = compactor
            .compact(
                &[
                    ChatMessage::system("system prompt"),
                    ChatMessage::user("short"),
                    ChatMessage::assistant("reply"),
                ],
                10,
            )
            .await
            .expect("turn compact should succeed");

        assert!(result.is_none());
        assert!(requests.lock().unwrap().is_empty());
    }
}
