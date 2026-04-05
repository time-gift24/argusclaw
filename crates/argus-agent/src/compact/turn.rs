use std::sync::Arc;

use argus_protocol::llm::{
    ChatMessage, ChatMessageMetadata, ChatMessageMetadataMode, CompletionRequest, LlmProvider, Role,
};
use async_trait::async_trait;

use crate::error::CompactError;

const USER_HISTORY_TOKEN_BUDGET: usize = 20_000;
const TURN_COMPACTION_PROMPT: &str = "\
Write a compact continuation message from the user's perspective.\n\
Summarize what I asked you to do, what you already discovered or completed,\n\
what context still matters, and what I want you to continue doing next.\n\
Write the summary as if it were written by the user in first person.\n\
Do not call any tools. Do not write from the assistant perspective.\n\
Respond only with the summary text.";

#[derive(Debug, Clone)]
pub struct TurnCompactResult {
    pub checkpoint_messages: Vec<ChatMessage>,
}

#[async_trait]
pub trait TurnCompactor: Send + Sync {
    async fn compact(
        &self,
        system_prompt: &str,
        history: &[ChatMessage],
        turn_messages: &[ChatMessage],
    ) -> Result<Option<TurnCompactResult>, CompactError>;

    fn name(&self) -> &'static str;
}

pub struct LlmTurnCompactor {
    provider: Arc<dyn LlmProvider>,
}

impl LlmTurnCompactor {
    pub fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
    }

    fn estimated_tokens(content: &str) -> usize {
        (content.len().saturating_add(3)) / 4
    }

    fn select_recent_user_inputs(
        history: &[ChatMessage],
        turn_messages: &[ChatMessage],
    ) -> Vec<ChatMessage> {
        let user_inputs: Vec<_> = history
            .iter()
            .chain(turn_messages.iter())
            .filter(|message| message.role == Role::User)
            .cloned()
            .collect();

        let mut selected = Vec::new();
        let mut used_budget = 0usize;
        for message in user_inputs.into_iter().rev() {
            let estimate = Self::estimated_tokens(&message.content);
            if !selected.is_empty() && used_budget.saturating_add(estimate) > USER_HISTORY_TOKEN_BUDGET
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
        system_prompt: &str,
        selected_history: &[ChatMessage],
        turn_messages: &[ChatMessage],
    ) -> Vec<ChatMessage> {
        let mut request_messages = Vec::with_capacity(
            selected_history
                .len()
                .saturating_add(turn_messages.len())
                .saturating_add(usize::from(!system_prompt.is_empty()))
                .saturating_add(1),
        );

        if !system_prompt.is_empty() {
            request_messages.push(ChatMessage::system(system_prompt));
        }
        request_messages.extend(selected_history.iter().cloned());
        request_messages.extend(turn_messages.iter().cloned());
        request_messages.push(
            ChatMessage::user(TURN_COMPACTION_PROMPT).with_metadata(ChatMessageMetadata {
                summary: false,
                mode: Some(ChatMessageMetadataMode::CompactionPrompt),
                synthetic: true,
                collapsed_by_default: true,
            }),
        );

        request_messages
    }
}

#[async_trait]
impl TurnCompactor for LlmTurnCompactor {
    async fn compact(
        &self,
        system_prompt: &str,
        history: &[ChatMessage],
        turn_messages: &[ChatMessage],
    ) -> Result<Option<TurnCompactResult>, CompactError> {
        let selected_history = Self::select_recent_user_inputs(history, turn_messages);
        if selected_history.is_empty() && turn_messages.is_empty() {
            return Ok(None);
        }

        let request = CompletionRequest::new(Self::build_request_messages(
            system_prompt,
            &selected_history,
            turn_messages,
        ));
        let response = self
            .provider
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
        checkpoint_messages.push(
            ChatMessage::user(summary).with_metadata(Self::summary_metadata()),
        );
        Ok(Some(TurnCompactResult { checkpoint_messages }))
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
    }

    impl CapturingSummaryProvider {
        fn new(requests: Arc<Mutex<Vec<CompletionRequest>>>, summary: impl Into<String>) -> Self {
            Self {
                requests,
                summary: summary.into(),
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
    }

    #[tokio::test]
    async fn compact_keeps_recent_user_inputs_in_reverse_chronological_order() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let compactor = LlmTurnCompactor::new(Arc::new(CapturingSummaryProvider::new(
            Arc::clone(&requests),
            "continue from here",
        )));

        let result = compactor
            .compact(
                "system prompt",
                &[
                    ChatMessage::user("oldest user"),
                    ChatMessage::assistant("oldest assistant"),
                    ChatMessage::user("newer user"),
                ],
                &[
                    ChatMessage::assistant("tool context"),
                    ChatMessage::user("latest user"),
                ],
            )
            .await
            .expect("turn compact should succeed")
            .expect("turn compact should produce a result");

        let checkpoint_contents: Vec<_> = result
            .checkpoint_messages
            .iter()
            .map(|message| message.content.as_str())
            .collect();
        assert_eq!(
            checkpoint_contents,
            vec!["latest user", "newer user", "oldest user", "continue from here"]
        );
    }

    #[tokio::test]
    async fn compact_includes_system_prompt_in_request_but_not_checkpoint_messages() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let compactor = LlmTurnCompactor::new(Arc::new(CapturingSummaryProvider::new(
            Arc::clone(&requests),
            "please continue the task",
        )));

        let result = compactor
            .compact(
                "You are a helpful assistant.",
                &[ChatMessage::user("older request")],
                &[ChatMessage::assistant("recent tool output")],
            )
            .await
            .expect("turn compact should succeed")
            .expect("turn compact should produce a result");

        let captured = requests.lock().unwrap();
        let request = captured.first().expect("provider should capture a request");
        assert_eq!(request.messages[0].role, Role::System);
        assert_eq!(request.messages[0].content, "You are a helpful assistant.");
        assert!(result
            .checkpoint_messages
            .iter()
            .all(|message| message.role != Role::System));
        assert_eq!(
            result
                .checkpoint_messages
                .last()
                .expect("summary message should exist")
                .role,
            Role::User
        );
    }

    #[tokio::test]
    async fn compact_marks_summary_as_synthetic_user_summary_and_uses_user_prompt() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let compactor = LlmTurnCompactor::new(Arc::new(CapturingSummaryProvider::new(
            Arc::clone(&requests),
            "I already asked you to inspect the latest tool result and continue.",
        )));

        let result = compactor
            .compact(
                "system prompt",
                &[ChatMessage::user("older request")],
                &[
                    ChatMessage::assistant("found one issue"),
                    ChatMessage::tool_result("tool-1", "echo", "{\"ok\":true}"),
                ],
            )
            .await
            .expect("turn compact should succeed")
            .expect("turn compact should produce a result");

        let request = requests
            .lock()
            .unwrap()
            .first()
            .expect("provider should capture a request")
            .clone();
        let prompt = request
            .messages
            .last()
            .expect("compaction prompt should be appended");
        assert_eq!(prompt.role, Role::User);
        assert!(prompt.content.contains("user's perspective"));
        assert!(prompt.content.contains("Do not call any tools"));
        assert!(prompt.content.contains("assistant perspective"));

        let summary = result
            .checkpoint_messages
            .last()
            .expect("summary message should exist");
        let metadata = summary
            .metadata
            .as_ref()
            .expect("summary should carry metadata");
        assert_eq!(summary.role, Role::User);
        assert!(metadata.summary);
        assert_eq!(metadata.mode, Some(ChatMessageMetadataMode::CompactionSummary));
        assert!(metadata.synthetic);
        assert!(metadata.collapsed_by_default);
    }
}
