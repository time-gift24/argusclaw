//! get_workflow_progress tool implementation.

use std::sync::Arc;

use argus_protocol::{NamedTool, RiskLevel, ToolDefinition, ToolError, ToolExecutionContext};
use async_trait::async_trait;

use crate::types::{GetWorkflowProgressArgs, GetWorkflowProgressResult};
use crate::WorkflowManager;

/// Tool for reading workflow progress.
pub struct GetWorkflowProgressTool {
    workflow_manager: Arc<WorkflowManager>,
}

impl GetWorkflowProgressTool {
    /// Create a new GetWorkflowProgressTool.
    pub fn new(workflow_manager: Arc<WorkflowManager>) -> Self {
        Self { workflow_manager }
    }
}

impl std::fmt::Debug for GetWorkflowProgressTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("GetWorkflowProgressTool")
    }
}

#[cfg(test)]
#[tokio::test]
async fn get_workflow_progress_returns_grouped_counts() {
    tests::get_workflow_progress_returns_grouped_counts_impl().await;
}

#[cfg(test)]
#[tokio::test]
async fn workflow_tool_get_workflow_progress_returns_grouped_counts() {
    tests::get_workflow_progress_returns_grouped_counts_impl().await;
}

#[async_trait]
impl NamedTool for GetWorkflowProgressTool {
    fn name(&self) -> &str {
        "get_workflow_progress"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Get the current progress for a persistent workflow execution.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "workflow_execution_id": {
                        "type": "string",
                        "description": "The workflow execution ID returned by start_workflow"
                    }
                },
                "required": ["workflow_execution_id"]
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: Arc<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError> {
        let args: GetWorkflowProgressArgs =
            serde_json::from_value(input).map_err(|error| ToolError::ExecutionFailed {
                tool_name: self.name().to_string(),
                reason: format!("invalid input: {error}"),
            })?;

        let execution_id = argus_repository::types::WorkflowId::new(args.workflow_execution_id);
        let progress = self
            .workflow_manager
            .get_workflow_progress(&execution_id)
            .await
            .map_err(|error| ToolError::ExecutionFailed {
                tool_name: self.name().to_string(),
                reason: error.to_string(),
            })?;

        let Some(progress) = progress else {
            return Ok(serde_json::json!({
                "workflow_execution_id": execution_id.to_string(),
                "status": "not_found",
            }));
        };

        serde_json::to_value(GetWorkflowProgressResult { progress }).map_err(|error| {
            ToolError::ExecutionFailed {
                tool_name: self.name().to_string(),
                reason: format!("failed to serialize response: {error}"),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use argus_protocol::{
        AgentId, LlmProvider, ProviderId, ProviderResolver, ThreadEvent, ThreadId,
        ToolExecutionContext,
    };
    use argus_repository::traits::{AgentRepository, JobRepository, WorkflowRepository};
    use argus_repository::types::{JobRecord, JobType, WorkflowId, WorkflowRecord, WorkflowStatus};
    use argus_repository::{ArgusSqlite, connect_path, migrate};
    use argus_template::TemplateManager;
    use argus_tool::ToolManager;
    use async_trait::async_trait;
    use tokio::sync::{broadcast, mpsc};
    use uuid::Uuid;

    use super::*;

    #[derive(Debug)]
    struct DummyProviderResolver;

    #[async_trait]
    impl ProviderResolver for DummyProviderResolver {
        async fn resolve(&self, _id: ProviderId) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
            unreachable!("resolver should not be called in workflow progress tests");
        }

        async fn default_provider(&self) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
            unreachable!("resolver should not be called in workflow progress tests");
        }

        async fn resolve_with_model(
            &self,
            _id: ProviderId,
            _model: &str,
        ) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
            unreachable!("resolver should not be called in workflow progress tests");
        }
    }

    async fn build_tool() -> (Arc<GetWorkflowProgressTool>, WorkflowId) {
        let db_path = std::env::temp_dir().join(format!("argus-progress-{}.sqlite", Uuid::new_v4()));
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

        let template_manager = Arc::new(TemplateManager::new(
            repo.clone() as Arc<dyn AgentRepository>,
            repo.clone(),
        ));
        let workflow_id = WorkflowId::new("wf-progress");
        repo.create_workflow_execution(&WorkflowRecord {
            id: workflow_id.clone(),
            name: "Progress".to_string(),
            status: WorkflowStatus::Running,
            template_id: None,
            template_version: None,
            initiating_thread_id: None,
        })
        .await
        .expect("create workflow");

        for (idx, status) in [
            WorkflowStatus::Succeeded,
            WorkflowStatus::Running,
            WorkflowStatus::Pending,
        ]
        .into_iter()
        .enumerate()
        {
            repo.create(&JobRecord {
                id: WorkflowId::new(format!("job-{idx}")),
                job_type: JobType::Workflow,
                name: format!("Node {idx}"),
                status,
                agent_id: AgentId::new(7),
                context: None,
                prompt: "Do work".to_string(),
                thread_id: None,
                group_id: Some(workflow_id.to_string()),
                node_key: Some(format!("node-{idx}")),
                depends_on: Vec::new(),
                cron_expr: None,
                scheduled_at: None,
                started_at: None,
                finished_at: None,
                parent_job_id: None,
                result: None,
            })
            .await
            .expect("create job");
        }

        let job_manager = Arc::new(crate::JobManager::with_job_repository(
            template_manager.clone(),
            Arc::new(DummyProviderResolver),
            Arc::new(ToolManager::new()),
            repo.clone() as Arc<dyn JobRepository>,
        ));
        let workflow_manager = Arc::new(WorkflowManager::new(
            template_manager,
            repo.clone() as Arc<dyn WorkflowRepository>,
            repo.clone() as Arc<dyn JobRepository>,
            job_manager,
        ));

        (Arc::new(GetWorkflowProgressTool::new(workflow_manager)), workflow_id)
    }

    fn test_ctx() -> Arc<ToolExecutionContext> {
        let (pipe_tx, _pipe_rx) = broadcast::channel::<ThreadEvent>(8);
        let (control_tx, _control_rx) = mpsc::unbounded_channel();
        Arc::new(ToolExecutionContext {
            thread_id: ThreadId::new(),
            pipe_tx,
            control_tx,
        })
    }

    pub(crate) async fn get_workflow_progress_returns_grouped_counts_impl() {
        let (tool, workflow_id) = build_tool().await;

        let response = tool
            .execute(
                serde_json::json!({
                    "workflow_execution_id": workflow_id.to_string(),
                }),
                test_ctx(),
            )
            .await
            .expect("tool should succeed");

        assert_eq!(response.pointer("/progress/total_nodes"), Some(&serde_json::json!(3)));
        assert_eq!(response.pointer("/progress/succeeded_nodes"), Some(&serde_json::json!(1)));
        assert_eq!(response.pointer("/progress/running_nodes"), Some(&serde_json::json!(1)));
        assert_eq!(response.pointer("/progress/pending_nodes"), Some(&serde_json::json!(1)));
    }
}
