use std::sync::Arc;

use argus_agent::execute_one_shot_thread;
use argus_mcp::McpRuntime;
use argus_protocol::llm::LlmProvider;
use argus_protocol::{AgentId, AgentRecord, ArgusError, LlmProviderId, Result, TokenUsage};
use serde::Serialize;

use crate::ArgusWing;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OneShotAgentSelector {
    Id(AgentId),
    DisplayName(String),
}

#[derive(Debug, Clone)]
pub struct OneShotRunRequest {
    pub agent: OneShotAgentSelector,
    pub prompt: String,
    pub system_prompt: Option<String>,
    pub model: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OneShotRunResult {
    pub agent_id: i64,
    pub agent_display_name: String,
    pub provider_id: i64,
    pub provider_model: String,
    pub assistant_message: String,
    pub token_usage: TokenUsage,
}

struct ResolvedOneShotProvider {
    provider_id: LlmProviderId,
    provider: Arc<dyn LlmProvider>,
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

impl ArgusWing {
    async fn resolve_one_shot_template(&self, agent: &OneShotAgentSelector) -> Result<AgentRecord> {
        let template = match agent {
            OneShotAgentSelector::Id(agent_id) => self.get_template(*agent_id).await?,
            OneShotAgentSelector::DisplayName(agent_name) => self
                .template_manager
                .find_by_display_name(agent_name)
                .await
                .map_err(|error| ArgusError::DatabaseError {
                    reason: error.to_string(),
                })?,
        };

        template.ok_or_else(|| ArgusError::DatabaseError {
            reason: match agent {
                OneShotAgentSelector::Id(agent_id) => {
                    format!("agent template not found: id={}", agent_id.inner())
                }
                OneShotAgentSelector::DisplayName(agent_name) => {
                    format!("agent template not found: {agent_name}")
                }
            },
        })
    }

    async fn resolve_one_shot_provider(
        &self,
        agent_record: &AgentRecord,
        requested_model: Option<&str>,
    ) -> Result<ResolvedOneShotProvider> {
        let requested_model = requested_model.or(agent_record.model_id.as_deref());

        match agent_record.provider_id {
            Some(provider_id) => {
                let provider_id = LlmProviderId::new(provider_id.inner());
                let provider = match requested_model {
                    Some(model) => {
                        self.provider_manager
                            .get_provider_with_model(&provider_id, model)
                            .await
                    }
                    None => self.provider_manager.get_provider(&provider_id).await,
                }?;
                Ok(ResolvedOneShotProvider {
                    provider_id,
                    provider,
                })
            }
            None => {
                let default_provider = self.provider_manager.get_default_provider_record().await?;
                let provider = match requested_model {
                    Some(model) => {
                        self.provider_manager
                            .get_provider_with_model(&default_provider.id, model)
                            .await
                    }
                    None => {
                        self.provider_manager
                            .get_provider(&default_provider.id)
                            .await
                    }
                }?;
                Ok(ResolvedOneShotProvider {
                    provider_id: default_provider.id,
                    provider,
                })
            }
        }
    }

    pub async fn run_one_shot(&self, request: OneShotRunRequest) -> Result<OneShotRunResult> {
        self.register_default_tools().await?;

        let template = self.resolve_one_shot_template(&request.agent).await?;
        let agent_record =
            prepare_agent_record_for_one_shot(&template, request.system_prompt.as_deref());
        let resolved_provider = self
            .resolve_one_shot_provider(&agent_record, request.model.as_deref())
            .await?;
        let provider_model = resolved_provider.provider.model_name().to_string();
        let mcp_tool_resolver: Arc<dyn argus_protocol::McpToolResolver> =
            Arc::new(McpRuntime::handle(&self.mcp_runtime));

        let one_shot = execute_one_shot_thread(
            resolved_provider.provider,
            agent_record.clone(),
            Arc::clone(&self.tool_manager),
            Some(mcp_tool_resolver),
            request.prompt,
        )
        .await
        .map_err(|error| match error {
            argus_agent::ThreadError::ProviderNotConfigured
            | argus_agent::ThreadError::CompactorNotConfigured
            | argus_agent::ThreadError::AgentRecordNotSet
            | argus_agent::ThreadError::SessionIdNotSet => ArgusError::ThreadBuildFailed {
                reason: error.to_string(),
            },
            _ => ArgusError::LlmError {
                reason: error.to_string(),
            },
        })?;

        Ok(OneShotRunResult {
            agent_id: agent_record.id.inner(),
            agent_display_name: agent_record.display_name,
            provider_id: resolved_provider.provider_id.into_inner(),
            provider_model,
            assistant_message: one_shot.assistant_message,
            token_usage: one_shot.token_usage,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;

    use sqlx::SqlitePool;

    use argus_protocol::{AgentId, AgentRecord, LlmProviderKind, LlmProviderRecord, SecretString};

    use super::*;

    async fn make_test_wing() -> Arc<ArgusWing> {
        let pool = SqlitePool::connect_lazy("sqlite::memory:")
            .expect("lazy sqlite pool should build for tests");
        argus_repository::migrate(&pool)
            .await
            .expect("test migrations should succeed");
        ArgusWing::with_pool(pool)
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
            .resolve_one_shot_template(&OneShotAgentSelector::Id(agent_id))
            .await
            .expect("template lookup by id should succeed");

        assert_eq!(template.id, agent_id);
        assert_eq!(template.display_name, "DB Selected Agent");
    }

    #[tokio::test]
    async fn resolve_one_shot_provider_reports_effective_default_provider_id() {
        let wing = make_test_wing().await;
        let provider_id = wing
            .upsert_provider(LlmProviderRecord {
                id: argus_protocol::LlmProviderId::new(0),
                kind: LlmProviderKind::OpenAiCompatible,
                display_name: "Default test provider".to_string(),
                api_key: SecretString::new("sk-test"),
                models: vec!["gpt-4o-mini".to_string()],
                model_config: HashMap::new(),
                default_model: "gpt-4o-mini".to_string(),
                base_url: "https://example.test/v1".to_string(),
                is_default: true,
                extra_headers: HashMap::new(),
                meta_data: HashMap::new(),
                secret_status: argus_protocol::ProviderSecretStatus::Ready,
            })
            .await
            .expect("provider should upsert");

        let resolved = wing
            .resolve_one_shot_provider(&AgentRecord::default(), None)
            .await
            .expect("default provider should resolve");

        assert_eq!(resolved.provider_id, provider_id);
        assert_eq!(resolved.provider.model_name(), "gpt-4o-mini");
    }
}
