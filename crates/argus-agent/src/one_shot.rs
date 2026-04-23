use std::sync::Arc;

use argus_protocol::llm::{ChatMessage, LlmProvider, Role};
use argus_protocol::{AgentRecord, McpToolResolver, SessionId, TokenUsage};
use argus_tool::ToolManager;

use crate::{LlmThreadCompactor, ThreadBuilder, ThreadError, TurnCancellation, TurnError};

#[derive(Debug, Clone)]
pub struct OneShotThreadResult {
    pub assistant_message: String,
    pub token_usage: TokenUsage,
}

fn extract_last_assistant_message(messages: &[ChatMessage]) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|message| message.role == Role::Assistant)
        .and_then(|message| {
            if !message.content.trim().is_empty() {
                Some(message.content.clone())
            } else {
                message.reasoning_content.clone()
            }
        })
}

/// Execute a single user prompt through the thread-owned turn lifecycle.
///
/// # Errors
///
/// Returns a [`ThreadError`] if the thread cannot be built, the turn fails, or
/// the completed turn does not produce an assistant reply.
pub async fn execute_one_shot_thread(
    provider: Arc<dyn LlmProvider>,
    agent_record: AgentRecord,
    tool_manager: Arc<ToolManager>,
    mcp_tool_resolver: Option<Arc<dyn McpToolResolver>>,
    prompt: String,
) -> Result<OneShotThreadResult, ThreadError> {
    let mut builder = ThreadBuilder::new()
        .provider(Arc::clone(&provider))
        .compactor(Arc::new(LlmThreadCompactor::new(provider)))
        .agent_record(Arc::new(agent_record))
        .tool_manager(tool_manager)
        .session_id(SessionId::new());

    if let Some(mcp_tool_resolver) = mcp_tool_resolver {
        builder = builder.mcp_tool_resolver(mcp_tool_resolver);
    }

    let mut thread = builder.build()?;
    let record = thread
        .execute_turn(prompt, None, TurnCancellation::new())
        .await?;
    let committed_messages: Vec<_> = thread.history_iter().cloned().collect();
    let assistant_message =
        extract_last_assistant_message(&committed_messages).ok_or_else(|| {
            ThreadError::TurnFailed(TurnError::BuildFailed(
                "turn completed without an assistant reply".to_string(),
            ))
        })?;

    Ok(OneShotThreadResult {
        assistant_message,
        token_usage: record.token_usage,
    })
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use argus_protocol::llm::{
        CompletionRequest, CompletionResponse, FinishReason, LlmError, LlmProvider,
    };
    use argus_protocol::{AgentRecord, llm::Role};
    use argus_tool::ToolManager;
    use async_trait::async_trait;
    use rust_decimal::Decimal;

    use super::*;

    struct RequestCapturingProvider {
        requests: Arc<Mutex<Vec<CompletionRequest>>>,
        response: CompletionResponse,
    }

    #[async_trait]
    impl LlmProvider for RequestCapturingProvider {
        fn model_name(&self) -> &str {
            "capturing"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            request: CompletionRequest,
        ) -> std::result::Result<CompletionResponse, LlmError> {
            self.requests
                .lock()
                .expect("request capture lock should be available")
                .push(request);
            Ok(self.response.clone())
        }
    }

    #[tokio::test]
    async fn execute_one_shot_thread_returns_reply_and_uses_agent_prompt() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let provider = Arc::new(RequestCapturingProvider {
            requests: Arc::clone(&requests),
            response: CompletionResponse {
                content: Some("task complete".to_string()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: 11,
                output_tokens: 7,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
        });

        let result = execute_one_shot_thread(
            provider,
            AgentRecord {
                display_name: "Runner".to_string(),
                system_prompt: "Use this prompt".to_string(),
                ..AgentRecord::default()
            },
            Arc::new(ToolManager::new()),
            None,
            "Do the task".to_string(),
        )
        .await
        .expect("one-shot execution should succeed");

        assert_eq!(result.assistant_message, "task complete");
        assert_eq!(result.token_usage.total_tokens, 18);

        let captured = requests
            .lock()
            .expect("request capture lock should be available");
        let request = captured
            .first()
            .expect("provider should receive one completion request");
        assert!(request
            .messages
            .iter()
            .any(|message| message.role == Role::System && message.content == "Use this prompt"));
        assert!(
            request
                .messages
                .iter()
                .any(|message| message.role == Role::User && message.content == "Do the task")
        );
    }
}
