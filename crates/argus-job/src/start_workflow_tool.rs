//! start_workflow tool implementation.

use std::sync::Arc;

use argus_protocol::{NamedTool, RiskLevel, ToolDefinition, ToolError, ToolExecutionContext};
use async_trait::async_trait;

use crate::types::{StartWorkflowArgs, StartWorkflowResult};
use crate::{InstantiateWorkflowInput, WorkflowManager};

/// Tool for starting persistent workflows.
pub struct StartWorkflowTool {
    workflow_manager: Arc<WorkflowManager>,
}

impl StartWorkflowTool {
    /// Create a new StartWorkflowTool.
    pub fn new(workflow_manager: Arc<WorkflowManager>) -> Self {
        Self { workflow_manager }
    }
}

impl std::fmt::Debug for StartWorkflowTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("StartWorkflowTool")
    }
}

#[cfg(test)]
#[tokio::test]
async fn start_workflow_returns_execution_id() {
    tests::start_workflow_returns_execution_id_impl().await;
}

#[cfg(test)]
#[tokio::test]
async fn workflow_tool_start_workflow_returns_execution_id() {
    tests::start_workflow_returns_execution_id_impl().await;
}

#[async_trait]
impl NamedTool for StartWorkflowTool {
    fn name(&self) -> &str {
        "start_workflow"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description:
                "Instantiate a persistent workflow from a template and return its execution ID."
                    .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "template_id": { "type": "string" },
                    "template_version": { "type": "integer" },
                    "extra_nodes": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "node_key": { "type": "string" },
                                "name": { "type": "string" },
                                "agent_id": { "type": "integer" },
                                "prompt": { "type": "string" },
                                "context": { "type": "string" },
                                "depends_on_keys": {
                                    "type": "array",
                                    "items": { "type": "string" }
                                }
                            },
                            "required": ["node_key", "name", "agent_id", "prompt"]
                        }
                    }
                },
                "required": ["template_id"]
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Medium
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: Arc<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError> {
        let args: StartWorkflowArgs =
            serde_json::from_value(input).map_err(|error| ToolError::ExecutionFailed {
                tool_name: self.name().to_string(),
                reason: format!("invalid input: {error}"),
            })?;

        let progress = self
            .workflow_manager
            .instantiate_workflow(InstantiateWorkflowInput {
                template_id: argus_repository::types::WorkflowTemplateId::new(args.template_id),
                template_version: args.template_version,
                initiating_thread_id: Some(ctx.thread_id),
                extra_nodes: args.extra_nodes,
            })
            .await
            .map_err(|error| ToolError::ExecutionFailed {
                tool_name: self.name().to_string(),
                reason: error.to_string(),
            })?;

        serde_json::to_value(StartWorkflowResult {
            workflow_execution_id: progress.workflow_id.to_string(),
            progress,
        })
        .map_err(|error| ToolError::ExecutionFailed {
            tool_name: self.name().to_string(),
            reason: format!("failed to serialize response: {error}"),
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use argus_protocol::llm::{
        CompletionRequest, CompletionResponse, FinishReason, LlmError, ToolCompletionRequest,
        ToolCompletionResponse,
    };
    use argus_protocol::{
        LlmProvider, ProviderId, ProviderResolver, ThreadEvent, ThreadId, ToolExecutionContext,
    };
    use argus_repository::traits::{AgentRepository, JobRepository, WorkflowRepository};
    use argus_repository::types::{
        WorkflowTemplateId, WorkflowTemplateNodeRecord, WorkflowTemplateRecord,
    };
    use argus_repository::{ArgusSqlite, connect_path, migrate};
    use argus_template::TemplateManager;
    use argus_tool::ToolManager;
    use async_trait::async_trait;
    use rust_decimal::Decimal;
    use tokio::sync::{broadcast, mpsc};
    use uuid::Uuid;

    use super::*;

    #[derive(Debug)]
    struct ImmediateProvider;

    #[async_trait]
    impl LlmProvider for ImmediateProvider {
        fn model_name(&self) -> &str {
            "workflow-tool-provider"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            unreachable!("complete is not used in workflow tool tests")
        }

        async fn complete_with_tools(
            &self,
            _request: ToolCompletionRequest,
        ) -> Result<ToolCompletionResponse, LlmError> {
            Ok(ToolCompletionResponse {
                content: Some("workflow step complete".to_string()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: 3,
                output_tokens: 5,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            })
        }
    }

    struct StaticProviderResolver {
        provider: Arc<dyn LlmProvider>,
    }

    #[async_trait]
    impl ProviderResolver for StaticProviderResolver {
        async fn resolve(&self, _id: ProviderId) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
            Ok(Arc::clone(&self.provider))
        }

        async fn default_provider(&self) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
            Ok(Arc::clone(&self.provider))
        }

        async fn resolve_with_model(
            &self,
            _id: ProviderId,
            _model: &str,
        ) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
            Ok(Arc::clone(&self.provider))
        }
    }

    async fn build_tool() -> (Arc<StartWorkflowTool>, ThreadId) {
        let db_path =
            std::env::temp_dir().join(format!("argus-start-workflow-{}.sqlite", Uuid::new_v4()));
        let pool = connect_path(&db_path).await.expect("create sqlite pool");
        migrate(&pool).await.expect("run migrations");
        let repo = Arc::new(ArgusSqlite::new(pool));

        let provider_id: i64 =
            sqlx::query_scalar("SELECT id FROM llm_providers ORDER BY id LIMIT 1")
                .fetch_one(repo.pool())
                .await
                .expect("default provider");

        sqlx::query(
            "INSERT INTO agents (id, display_name, description, version, provider_id, model_id, system_prompt, tool_names, max_tokens, temperature, thinking_config)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )
        .bind(7_i64)
        .bind("Workflow Agent")
        .bind("Workflow agent")
        .bind("1.0.0")
        .bind(provider_id)
        .bind(Option::<String>::None)
        .bind("You are a workflow agent.")
        .bind("[]")
        .bind(Option::<i64>::None)
        .bind(Option::<i64>::None)
        .bind(r#"{"type":"disabled","clear_thinking":false}"#)
        .execute(repo.pool())
        .await
        .expect("seed agent");

        let thread_id = ThreadId::new();
        sqlx::query(
            "INSERT INTO threads (id, provider_id, title, token_count, turn_count, session_id, template_id)
             VALUES (?1, ?2, ?3, 0, 0, NULL, NULL)",
        )
        .bind(thread_id.to_string())
        .bind(provider_id)
        .bind("workflow start thread")
        .execute(repo.pool())
        .await
        .expect("seed thread");

        let template_manager = Arc::new(TemplateManager::new(
            repo.clone() as Arc<dyn AgentRepository>,
            repo.clone(),
        ));
        repo.create_workflow_template(&WorkflowTemplateRecord {
            id: WorkflowTemplateId::new("tpl-1"),
            name: "Demo".to_string(),
            version: 1,
            description: "Demo workflow".to_string(),
        })
        .await
        .expect("create template");
        repo.create_workflow_template_node(&WorkflowTemplateNodeRecord {
            template_id: WorkflowTemplateId::new("tpl-1"),
            template_version: 1,
            node_key: "collect".to_string(),
            name: "Collect".to_string(),
            agent_id: argus_protocol::AgentId::new(7),
            prompt: "Collect context".to_string(),
            context: None,
            depends_on_keys: vec![],
        })
        .await
        .expect("create template node");

        let provider: Arc<dyn LlmProvider> = Arc::new(ImmediateProvider);
        let job_manager = Arc::new(crate::JobManager::with_job_repository(
            template_manager.clone(),
            Arc::new(StaticProviderResolver { provider }),
            Arc::new(ToolManager::new()),
            repo.clone() as Arc<dyn JobRepository>,
        ));
        let workflow_manager = Arc::new(WorkflowManager::new(
            template_manager,
            repo.clone() as Arc<dyn WorkflowRepository>,
            repo.clone() as Arc<dyn JobRepository>,
            job_manager,
        ));

        (Arc::new(StartWorkflowTool::new(workflow_manager)), thread_id)
    }

    fn test_ctx(thread_id: ThreadId) -> Arc<ToolExecutionContext> {
        let (pipe_tx, _pipe_rx) = broadcast::channel::<ThreadEvent>(8);
        let (control_tx, _control_rx) = mpsc::unbounded_channel();
        Arc::new(ToolExecutionContext {
            thread_id,
            pipe_tx,
            control_tx,
        })
    }

    pub(crate) async fn start_workflow_returns_execution_id_impl() {
        let (tool, thread_id) = build_tool().await;

        let response = tool
            .execute(
                serde_json::json!({
                    "template_id": "tpl-1",
                    "extra_nodes": [],
                }),
                test_ctx(thread_id),
            )
            .await
            .expect("tool should succeed");

        assert!(response.get("workflow_execution_id").is_some());
        assert_eq!(response.pointer("/progress/total_nodes"), Some(&serde_json::json!(1)));
    }

    #[tokio::test]
    async fn definition_matches_start_workflow_input_shape() {
        let (tool, _thread_id) = build_tool().await;
        let definition = tool.definition();

        assert_eq!(
            definition.parameters.pointer("/properties/template_version/type"),
            Some(&serde_json::json!("integer"))
        );
        assert_eq!(
            definition
                .parameters
                .pointer("/properties/extra_nodes/items/properties/node_key/type"),
            Some(&serde_json::json!("string"))
        );
        assert_eq!(
            definition
                .parameters
                .pointer("/properties/extra_nodes/items/properties/agent_id/type"),
            Some(&serde_json::json!("integer"))
        );
    }
}
