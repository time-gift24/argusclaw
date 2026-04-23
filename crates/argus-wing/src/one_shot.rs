use std::sync::Arc;

use argus_agent::{LlmThreadCompactor, ThreadBuilder, TurnCancellation};
use argus_mcp::McpRuntime;
use argus_protocol::llm::{ChatMessage, LlmProvider};
use argus_protocol::{
    AgentId, AgentRecord, ArgusError, LlmProviderId, Result, SessionId, TokenUsage,
};
use argus_tool::ToolManager;
use serde::Serialize;

use crate::{ArgusWing, DEFAULT_AGENT_DISPLAY_NAME};

#[derive(Debug, Clone)]
pub struct OneShotRunRequest {
    pub agent_id: Option<AgentId>,
    pub agent_name: Option<String>,
    pub prompt: String,
    pub system_prompt: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OneShotRunResult {
    pub agent_id: i64,
    pub agent_display_name: String,
    pub provider_id: Option<i64>,
    pub provider_model: String,
    pub assistant_message: String,
    pub token_usage: TokenUsage,
}

pub(crate) fn prepare_agent_record_for_one_shot(
    template: &AgentRecord,
    system_prompt_override: Option<&str>,
) -> AgentRecord {
    let mut record = template.clone();
    if let Some(system_prompt_override) = system_prompt_override {
        record.system_prompt = system_prompt_override.to_string();
    }
    record
}

pub(crate) fn extract_last_assistant_message(messages: &[ChatMessage]) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|message| message.role == argus_protocol::llm::Role::Assistant)
        .and_then(|message| {
            if !message.content.trim().is_empty() {
                Some(message.content.clone())
            } else {
                message.reasoning_content.clone()
            }
        })
}

pub(crate) async fn execute_one_shot_turn(
    provider: Arc<dyn LlmProvider>,
    agent_record: AgentRecord,
    tool_manager: Arc<ToolManager>,
    mcp_tool_resolver: Option<Arc<dyn argus_protocol::McpToolResolver>>,
    prompt: String,
) -> Result<OneShotRunResult> {
    let provider_model = provider.model_name().to_string();
    let mut builder = ThreadBuilder::new()
        .provider(Arc::clone(&provider))
        .compactor(Arc::new(LlmThreadCompactor::new(provider)))
        .agent_record(Arc::new(agent_record.clone()))
        .tool_manager(tool_manager)
        .session_id(SessionId::new());

    if let Some(mcp_tool_resolver) = mcp_tool_resolver {
        builder = builder.mcp_tool_resolver(mcp_tool_resolver);
    }

    let mut thread = builder
        .build()
        .map_err(|error| ArgusError::ThreadBuildFailed {
            reason: error.to_string(),
        })?;

    let record = thread
        .execute_turn(prompt, None, TurnCancellation::new())
        .await
        .map_err(|error| ArgusError::LlmError {
            reason: error.to_string(),
        })?;

    let committed_messages: Vec<_> = thread.history_iter().cloned().collect();
    let assistant_message =
        extract_last_assistant_message(&committed_messages).ok_or_else(|| {
            ArgusError::LlmError {
                reason: "Turn completed without an assistant reply".to_string(),
            }
        })?;

    Ok(OneShotRunResult {
        agent_id: agent_record.id.inner(),
        agent_display_name: agent_record.display_name,
        provider_id: agent_record
            .provider_id
            .map(|provider_id| provider_id.inner()),
        provider_model,
        assistant_message,
        token_usage: record.token_usage,
    })
}

impl ArgusWing {
    async fn resolve_one_shot_template(
        &self,
        agent_id: Option<AgentId>,
        agent_name: Option<&str>,
    ) -> Result<AgentRecord> {
        let template = match (agent_id, agent_name) {
            (Some(agent_id), _) => self.get_template(agent_id).await?,
            (None, Some(agent_name)) => self
                .template_manager
                .find_by_display_name(agent_name)
                .await
                .map_err(|error| ArgusError::DatabaseError {
                    reason: error.to_string(),
                })?,
            (None, None) => self.get_default_template().await?,
        };

        template.ok_or_else(|| ArgusError::DatabaseError {
            reason: match (agent_id, agent_name) {
                (Some(agent_id), _) => format!("agent template not found: id={}", agent_id.inner()),
                (None, Some(agent_name)) => format!("agent template not found: {agent_name}"),
                (None, None) => {
                    format!("agent template not found: {}", DEFAULT_AGENT_DISPLAY_NAME)
                }
            },
        })
    }

    async fn resolve_one_shot_provider(
        &self,
        agent_record: &AgentRecord,
        requested_model: Option<&str>,
    ) -> Result<Arc<dyn LlmProvider>> {
        let requested_model = requested_model.or(agent_record.model_id.as_deref());

        match agent_record.provider_id {
            Some(provider_id) => {
                let provider_id = LlmProviderId::new(provider_id.inner());
                match requested_model {
                    Some(model) => {
                        self.provider_manager
                            .get_provider_with_model(&provider_id, model)
                            .await
                    }
                    None => self.provider_manager.get_provider(&provider_id).await,
                }
            }
            None => match requested_model {
                Some(model) => {
                    let default_provider =
                        self.provider_manager.get_default_provider_record().await?;
                    self.provider_manager
                        .get_provider_with_model(&default_provider.id, model)
                        .await
                }
                None => self.provider_manager.get_default_provider().await,
            },
        }
    }

    pub async fn run_one_shot(&self, request: OneShotRunRequest) -> Result<OneShotRunResult> {
        self.register_default_tools().await?;

        let template = self
            .resolve_one_shot_template(request.agent_id, request.agent_name.as_deref())
            .await?;
        let agent_record =
            prepare_agent_record_for_one_shot(&template, request.system_prompt.as_deref());
        let provider = self
            .resolve_one_shot_provider(&agent_record, request.model.as_deref())
            .await?;
        let mcp_tool_resolver: Arc<dyn argus_protocol::McpToolResolver> =
            Arc::new(McpRuntime::handle(&self.mcp_runtime));

        execute_one_shot_turn(
            provider,
            agent_record,
            Arc::clone(&self.tool_manager),
            Some(mcp_tool_resolver),
            request.prompt,
        )
        .await
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use async_trait::async_trait;
    use rust_decimal::Decimal;
    use sqlx::SqlitePool;

    use argus_protocol::llm::{
        ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LlmError, LlmProvider,
        Role,
    };
    use argus_protocol::{AgentId, AgentRecord};
    use argus_tool::ToolManager;

    use super::*;

    struct RequestCapturingProvider {
        requests: Arc<Mutex<Vec<CompletionRequest>>>,
        response: CompletionResponse,
    }

    impl RequestCapturingProvider {
        fn new(requests: Arc<Mutex<Vec<CompletionRequest>>>, response: CompletionResponse) -> Self {
            Self { requests, response }
        }
    }

    async fn make_test_wing() -> Arc<ArgusWing> {
        let pool = SqlitePool::connect_lazy("sqlite::memory:")
            .expect("lazy sqlite pool should build for tests");
        argus_repository::migrate(&pool)
            .await
            .expect("test migrations should succeed");
        ArgusWing::with_pool(pool)
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

    #[test]
    fn prepare_agent_record_for_one_shot_overrides_system_prompt_and_preserves_agent_identity() {
        let template = AgentRecord {
            display_name: "Researcher".to_string(),
            description: "Research agent".to_string(),
            system_prompt: "Original system prompt".to_string(),
            tool_names: vec!["shell".to_string()],
            ..AgentRecord::default()
        };

        let record = prepare_agent_record_for_one_shot(&template, Some("Override prompt"));

        assert_eq!(record.display_name, template.display_name);
        assert_eq!(record.description, template.description);
        assert_eq!(record.tool_names, template.tool_names);
        assert_eq!(record.system_prompt, "Override prompt");
    }

    #[test]
    fn extract_last_assistant_message_prefers_content_then_reasoning() {
        let messages = vec![
            ChatMessage::assistant_with_reasoning("", Some("fallback reasoning".to_string())),
            ChatMessage::assistant_with_reasoning(
                "final answer",
                Some("ignored reasoning".to_string()),
            ),
        ];

        assert_eq!(
            extract_last_assistant_message(&messages).as_deref(),
            Some("final answer")
        );
    }

    #[tokio::test]
    async fn resolve_one_shot_template_prefers_agent_id_lookup_from_database() {
        let wing = make_test_wing().await;
        let agent_id = wing
            .upsert_template(AgentRecord {
                id: AgentId::new(0),
                display_name: "DB Selected Agent".to_string(),
                description: "Picked by id".to_string(),
                version: "1.0.0".to_string(),
                provider_id: None,
                model_id: None,
                system_prompt: "Use the database selected agent.".to_string(),
                tool_names: vec![],
                subagent_names: vec![],
                max_tokens: None,
                temperature: None,
                thinking_config: None,
            })
            .await
            .expect("agent template should upsert");

        let template = wing
            .resolve_one_shot_template(Some(agent_id), None)
            .await
            .expect("template lookup by id should succeed");

        assert_eq!(template.id, agent_id);
        assert_eq!(template.display_name, "DB Selected Agent");
    }

    #[tokio::test]
    async fn execute_one_shot_turn_returns_reply_and_injects_override_prompt() {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let provider = Arc::new(RequestCapturingProvider::new(
            Arc::clone(&requests),
            CompletionResponse {
                content: Some("task complete".to_string()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: 11,
                output_tokens: 7,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
        ));
        let agent = prepare_agent_record_for_one_shot(
            &AgentRecord {
                display_name: "Runner".to_string(),
                system_prompt: "Template prompt".to_string(),
                ..AgentRecord::default()
            },
            Some("Override prompt"),
        );

        let result: OneShotRunResult = execute_one_shot_turn(
            provider,
            agent,
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
            .any(|message| message.role == Role::System && message.content == "Override prompt"));
        assert!(request
            .messages
            .iter()
            .any(|message| message.role == Role::User && message.content == "Do the task"));
    }
}
