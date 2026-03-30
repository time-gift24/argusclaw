//! Unified scheduler tool implementation.

use std::sync::Arc;

use argus_protocol::{
    AgentId, NamedTool, RiskLevel, ThreadControlEvent, ThreadEvent, ThreadJobResult,
    ToolDefinition, ToolError, ToolExecutionContext,
};
use argus_template::TemplateManager;
use async_trait::async_trait;
use tokio::sync::oneshot;
use uuid::Uuid;

use crate::job_manager::{JobLookup, JobManager};
use crate::types::{
    GetWorkflowProgressResult, JobResult, SchedulerToolArgs, StartWorkflowResult,
    SubagentSummary,
};
use crate::{AppendWorkflowNode, InstantiateWorkflowInput, WorkflowManager};

const TOOL_NAME: &str = "scheduler";

/// Tool for listing subagents, dispatching jobs, and orchestrating workflows.
pub struct SchedulerTool {
    template_manager: Arc<TemplateManager>,
    job_manager: Arc<JobManager>,
    workflow_manager: Arc<WorkflowManager>,
}

impl SchedulerTool {
    /// Create a new SchedulerTool.
    pub fn new(
        template_manager: Arc<TemplateManager>,
        job_manager: Arc<JobManager>,
        workflow_manager: Arc<WorkflowManager>,
    ) -> Self {
        Self {
            template_manager,
            job_manager,
            workflow_manager,
        }
    }

    fn definition_parameters() -> serde_json::Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": [
                        "list_subagents",
                        "dispatch_job",
                        "get_job_result",
                        "start_workflow",
                        "get_workflow_progress"
                    ],
                    "description": "Scheduler action to perform"
                },
                "prompt": {
                    "type": "string",
                    "description": "The prompt/task description for dispatch_job"
                },
                "agent_id": {
                    "type": "number",
                    "description": "The agent ID to use for dispatch_job"
                },
                "context": {
                    "type": "object",
                    "description": "Optional context JSON for dispatch_job"
                },
                "job_id": {
                    "type": "string",
                    "description": "The job ID returned by scheduler with action=dispatch_job"
                },
                "consume": {
                    "type": "boolean",
                    "description": "Whether get_job_result should consume the completed queued result"
                },
                "template_id": {
                    "type": "string",
                    "description": "The workflow template ID for start_workflow"
                },
                "template_version": {
                    "type": "integer",
                    "description": "Optional workflow template version for start_workflow"
                },
                "extra_nodes": {
                    "type": "array",
                    "description": "Optional append-only workflow nodes for start_workflow",
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
                },
                "workflow_execution_id": {
                    "type": "string",
                    "description": "The workflow execution ID returned by scheduler with action=start_workflow"
                }
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }

    fn serialize_job_result(result: &ThreadJobResult) -> Result<serde_json::Value, ToolError> {
        serde_json::to_value(JobResult {
            success: result.success,
            message: result.message.clone(),
            token_usage: result.token_usage.clone(),
            agent_id: result.agent_id,
            agent_display_name: result.agent_display_name.clone(),
            agent_description: result.agent_description.clone(),
        })
        .map_err(|error| ToolError::ExecutionFailed {
            tool_name: TOOL_NAME.to_string(),
            reason: format!("failed to serialize job result: {error}"),
        })
    }

    async fn claim_queued_runtime_result(
        ctx: &ToolExecutionContext,
        job_id: &str,
    ) -> Result<(), ToolError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        if let Err(error) = ctx
            .control_tx
            .send(ThreadControlEvent::ClaimQueuedJobResult {
                job_id: job_id.to_string(),
                reply_tx,
            })
        {
            tracing::warn!(job_id, "failed to enqueue queued-job claim: {error}");
            return Ok(());
        }

        if let Err(error) = reply_rx.await {
            tracing::warn!(job_id, "queued-job claim reply dropped: {error}");
        }

        Ok(())
    }

    fn lookup_response(job_id: &str, lookup: JobLookup) -> Result<serde_json::Value, ToolError> {
        match lookup {
            JobLookup::NotFound => Ok(serde_json::json!({
                "job_id": job_id,
                "status": "not_found",
            })),
            JobLookup::Pending => Ok(serde_json::json!({
                "job_id": job_id,
                "status": "pending",
            })),
            JobLookup::Completed(result) => Ok(serde_json::json!({
                "job_id": result.job_id,
                "status": "completed",
                "result": Self::serialize_job_result(&result)?,
            })),
            JobLookup::Consumed(result) => Ok(serde_json::json!({
                "job_id": result.job_id,
                "status": "consumed",
                "result": Self::serialize_job_result(&result)?,
            })),
        }
    }

    async fn execute_list_subagents(&self) -> Result<serde_json::Value, ToolError> {
        let agent_id = argus_agent::tool_context::current_agent_id().ok_or_else(|| {
            ToolError::ExecutionFailed {
                tool_name: TOOL_NAME.to_string(),
                reason: "current agent_id not available".to_string(),
            }
        })?;

        let subagents = self
            .template_manager
            .list_subagents(agent_id)
            .await
            .map_err(|error| ToolError::ExecutionFailed {
                tool_name: TOOL_NAME.to_string(),
                reason: error.to_string(),
            })?;

        let result: Vec<_> = subagents
            .into_iter()
            .map(|agent| SubagentSummary {
                agent_id: agent.id,
                display_name: agent.display_name,
                description: agent.description,
            })
            .collect();

        serde_json::to_value(result).map_err(|error| ToolError::ExecutionFailed {
            tool_name: TOOL_NAME.to_string(),
            reason: format!("failed to serialize subagent list: {error}"),
        })
    }

    async fn execute_dispatch_job(
        &self,
        prompt: String,
        agent_id: AgentId,
        context: Option<serde_json::Value>,
        ctx: &ToolExecutionContext,
    ) -> Result<serde_json::Value, ToolError> {
        let job_id = Uuid::new_v4().to_string();

        tracing::info!(
            "scheduler dispatch_job called: job_id={}, prompt_len={}, agent_id={:?}",
            job_id,
            prompt.len(),
            agent_id
        );

        let dispatch_event = ThreadEvent::JobDispatched {
            thread_id: ctx.thread_id,
            job_id: job_id.clone(),
            agent_id,
            prompt: prompt.clone(),
            context: context.clone(),
        };
        if let Err(error) = ctx.pipe_tx.send(dispatch_event) {
            tracing::warn!("failed to send JobDispatched event: {}", error);
        }

        self.job_manager
            .dispatch_job(
                ctx.thread_id,
                job_id.clone(),
                agent_id,
                prompt,
                context,
                ctx.pipe_tx.clone(),
                ctx.control_tx.clone(),
            )
            .await
            .map_err(|error| ToolError::ExecutionFailed {
                tool_name: TOOL_NAME.to_string(),
                reason: error.to_string(),
            })?;

        Ok(serde_json::json!({
            "job_id": job_id,
            "status": "dispatched",
        }))
    }

    async fn execute_get_job_result(
        &self,
        job_id: String,
        consume: Option<bool>,
        ctx: &ToolExecutionContext,
    ) -> Result<serde_json::Value, ToolError> {
        let consume = consume.unwrap_or(false);
        let lookup = self
            .job_manager
            .get_job_result_status(ctx.thread_id, &job_id, false);

        if consume && matches!(lookup, JobLookup::Completed(_)) {
            Self::claim_queued_runtime_result(ctx, &job_id).await?;
            let consumed_lookup =
                self.job_manager
                    .get_job_result_status(ctx.thread_id, &job_id, true);
            return Self::lookup_response(&job_id, consumed_lookup);
        }

        Self::lookup_response(&job_id, lookup)
    }

    async fn execute_start_workflow(
        &self,
        template_id: String,
        template_version: Option<i64>,
        extra_nodes: Vec<AppendWorkflowNode>,
        ctx: &ToolExecutionContext,
    ) -> Result<serde_json::Value, ToolError> {
        let progress = self
            .workflow_manager
            .instantiate_workflow(InstantiateWorkflowInput {
                template_id: argus_repository::types::WorkflowTemplateId::new(template_id),
                template_version,
                initiating_thread_id: Some(ctx.thread_id),
                extra_nodes,
            })
            .await
            .map_err(|error| ToolError::ExecutionFailed {
                tool_name: TOOL_NAME.to_string(),
                reason: error.to_string(),
            })?;

        serde_json::to_value(StartWorkflowResult {
            workflow_execution_id: progress.workflow_id.to_string(),
            progress,
        })
        .map_err(|error| ToolError::ExecutionFailed {
            tool_name: TOOL_NAME.to_string(),
            reason: format!("failed to serialize response: {error}"),
        })
    }

    async fn execute_get_workflow_progress(
        &self,
        workflow_execution_id: String,
        ctx: &ToolExecutionContext,
    ) -> Result<serde_json::Value, ToolError> {
        let execution_id = argus_repository::types::WorkflowId::new(workflow_execution_id);
        let belongs_to_thread = self
            .workflow_manager
            .workflow_belongs_to_thread(&execution_id, ctx.thread_id)
            .await
            .map_err(|error| ToolError::ExecutionFailed {
                tool_name: TOOL_NAME.to_string(),
                reason: error.to_string(),
            })?;
        if !belongs_to_thread {
            return Ok(serde_json::json!({
                "workflow_execution_id": execution_id.to_string(),
                "status": "not_found",
            }));
        }

        let progress = self
            .workflow_manager
            .get_workflow_progress(&execution_id)
            .await
            .map_err(|error| ToolError::ExecutionFailed {
                tool_name: TOOL_NAME.to_string(),
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
                tool_name: TOOL_NAME.to_string(),
                reason: format!("failed to serialize response: {error}"),
            }
        })
    }
}

impl std::fmt::Debug for SchedulerTool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SchedulerTool")
    }
}

#[async_trait]
impl NamedTool for SchedulerTool {
    fn name(&self) -> &str {
        TOOL_NAME
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name().to_string(),
            description: "Unified scheduler tool for listing subagents, dispatching background jobs, and starting or querying persistent workflows.".to_string(),
            parameters: Self::definition_parameters(),
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
        let args: SchedulerToolArgs =
            serde_json::from_value(input).map_err(|error| ToolError::ExecutionFailed {
                tool_name: TOOL_NAME.to_string(),
                reason: format!("invalid input: {error}"),
            })?;

        match args {
            SchedulerToolArgs::ListSubagents => self.execute_list_subagents().await,
            SchedulerToolArgs::DispatchJob {
                prompt,
                agent_id,
                context,
            } => self.execute_dispatch_job(prompt, agent_id, context, &ctx).await,
            SchedulerToolArgs::GetJobResult { job_id, consume } => {
                self.execute_get_job_result(job_id, consume, &ctx).await
            }
            SchedulerToolArgs::StartWorkflow {
                template_id,
                template_version,
                extra_nodes,
            } => {
                self.execute_start_workflow(template_id, template_version, extra_nodes, &ctx)
                    .await
            }
            SchedulerToolArgs::GetWorkflowProgress {
                workflow_execution_id,
            } => self
                .execute_get_workflow_progress(workflow_execution_id, &ctx)
                .await,
        }
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
        AgentId, AgentRecord, AgentType, LlmProvider, ProviderId, ProviderResolver, ThreadId,
        ThinkingConfig, ToolExecutionContext,
    };
    use argus_repository::traits::{AgentRepository, JobRepository, WorkflowRepository};
    use argus_repository::types::{
        JobRecord, JobType, WorkflowId, WorkflowRecord, WorkflowStatus, WorkflowTemplateId,
        WorkflowTemplateNodeRecord, WorkflowTemplateRecord,
    };
    use argus_repository::{ArgusSqlite, connect_path, migrate};
    use argus_template::TemplateManager;
    use argus_tool::ToolManager;
    use async_trait::async_trait;
    use rust_decimal::Decimal;
    use sqlx::SqlitePool;
    use tokio::sync::{broadcast, mpsc};
    use uuid::Uuid;

    use super::*;

    #[derive(Debug)]
    struct DummyProviderResolver;

    #[async_trait]
    impl ProviderResolver for DummyProviderResolver {
        async fn resolve(&self, _id: ProviderId) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
            unreachable!("resolver should not be called in scheduler tests")
        }

        async fn default_provider(&self) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
            unreachable!("resolver should not be called in scheduler tests")
        }

        async fn resolve_with_model(
            &self,
            _id: ProviderId,
            _model: &str,
        ) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
            unreachable!("resolver should not be called in scheduler tests")
        }
    }

    #[derive(Debug)]
    struct ImmediateProvider;

    #[async_trait]
    impl LlmProvider for ImmediateProvider {
        fn model_name(&self) -> &str {
            "scheduler-test-provider"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            unreachable!("complete is not used in scheduler tool tests")
        }

        async fn complete_with_tools(
            &self,
            _request: ToolCompletionRequest,
        ) -> Result<ToolCompletionResponse, LlmError> {
            Ok(ToolCompletionResponse {
                content: Some("scheduler step complete".to_string()),
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

    fn test_ctx(thread_id: ThreadId) -> Arc<ToolExecutionContext> {
        let (pipe_tx, _pipe_rx) = broadcast::channel::<ThreadEvent>(8);
        let (control_tx, _control_rx) = mpsc::unbounded_channel();
        Arc::new(ToolExecutionContext {
            thread_id,
            pipe_tx,
            control_tx,
        })
    }

    fn test_scheduler_tool() -> Arc<SchedulerTool> {
        let pool = SqlitePool::connect_lazy("sqlite::memory:")
            .expect("lazy sqlite pool should build for tests");
        let sqlite = Arc::new(ArgusSqlite::new(pool));
        let template_manager = Arc::new(TemplateManager::new(
            sqlite.clone() as Arc<dyn AgentRepository>,
            sqlite.clone(),
        ));
        let job_manager = Arc::new(JobManager::new(
            template_manager.clone(),
            Arc::new(DummyProviderResolver),
            Arc::new(ToolManager::new()),
            Arc::new(argus_agent::CompactorManager::with_defaults()),
            std::env::temp_dir().join("argus-job-tests"),
        ));
        let workflow_manager = Arc::new(WorkflowManager::new(
            template_manager.clone(),
            sqlite.clone() as Arc<dyn WorkflowRepository>,
            sqlite as Arc<dyn JobRepository>,
            job_manager.clone(),
        ));

        Arc::new(SchedulerTool::new(
            template_manager,
            job_manager,
            workflow_manager,
        ))
    }

    fn completed_job(job_id: &str) -> ThreadJobResult {
        ThreadJobResult {
            job_id: job_id.to_string(),
            success: true,
            message: "finished".to_string(),
            token_usage: None,
            agent_id: AgentId::new(8),
            agent_display_name: "Worker".to_string(),
            agent_description: "Does background work".to_string(),
        }
    }

    async fn build_start_workflow_tool() -> (Arc<SchedulerTool>, ThreadId) {
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
            agent_id: AgentId::new(7),
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
            template_manager.clone(),
            repo.clone() as Arc<dyn WorkflowRepository>,
            repo as Arc<dyn JobRepository>,
            job_manager.clone(),
        ));

        (
            Arc::new(SchedulerTool::new(
                template_manager,
                job_manager,
                workflow_manager,
            )),
            thread_id,
        )
    }

    async fn build_progress_tool() -> (Arc<SchedulerTool>, WorkflowId, ThreadId) {
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

        let thread_id = ThreadId::new();
        sqlx::query(
            "INSERT INTO threads (id, provider_id, title, token_count, turn_count, session_id, template_id)
             VALUES (?1, ?2, ?3, 0, 0, NULL, NULL)",
        )
        .bind(thread_id.to_string())
        .bind(provider_id)
        .bind("workflow progress thread")
        .execute(repo.pool())
        .await
        .expect("seed thread");

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
            initiating_thread_id: Some(thread_id),
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
            template_manager.clone(),
            repo.clone() as Arc<dyn WorkflowRepository>,
            repo as Arc<dyn JobRepository>,
            job_manager.clone(),
        ));

        (
            Arc::new(SchedulerTool::new(
                template_manager,
                job_manager,
                workflow_manager,
            )),
            workflow_id,
            thread_id,
        )
    }

    async fn build_list_subagents_tool() -> (Arc<SchedulerTool>, AgentId, AgentId) {
        let db_path =
            std::env::temp_dir().join(format!("argus-list-subagents-{}.sqlite", Uuid::new_v4()));
        let pool = connect_path(&db_path).await.expect("create sqlite pool");
        migrate(&pool).await.expect("run migrations");
        let repo = Arc::new(ArgusSqlite::new(pool));
        let template_manager = Arc::new(TemplateManager::new(
            repo.clone() as Arc<dyn AgentRepository>,
            repo.clone(),
        ));

        let parent_id = template_manager
            .upsert(AgentRecord {
                id: AgentId::new(0),
                display_name: "Parent Agent".to_string(),
                description: "Parent".to_string(),
                version: "1.0.0".to_string(),
                provider_id: None,
                model_id: None,
                system_prompt: "You are a parent agent.".to_string(),
                tool_names: vec![],
                max_tokens: None,
                temperature: None,
                thinking_config: Some(ThinkingConfig::enabled()),
                parent_agent_id: None,
                agent_type: AgentType::Standard,
            })
            .await
            .expect("create parent agent");
        let child_id = template_manager
            .upsert(AgentRecord {
                id: AgentId::new(0),
                display_name: "Child Agent".to_string(),
                description: "Child".to_string(),
                version: "1.0.0".to_string(),
                provider_id: None,
                model_id: None,
                system_prompt: "You are a child agent.".to_string(),
                tool_names: vec![],
                max_tokens: None,
                temperature: None,
                thinking_config: Some(ThinkingConfig::enabled()),
                parent_agent_id: Some(parent_id),
                agent_type: AgentType::Standard,
            })
            .await
            .expect("create child agent");

        let job_manager = Arc::new(JobManager::new(
            template_manager.clone(),
            Arc::new(DummyProviderResolver),
            Arc::new(ToolManager::new()),
            Arc::new(argus_agent::CompactorManager::with_defaults()),
            std::env::temp_dir().join("argus-job-tests"),
        ));
        let workflow_manager = Arc::new(WorkflowManager::new(
            template_manager.clone(),
            repo.clone() as Arc<dyn WorkflowRepository>,
            repo as Arc<dyn JobRepository>,
            job_manager.clone(),
        ));

        (
            Arc::new(SchedulerTool::new(
                template_manager,
                job_manager,
                workflow_manager,
            )),
            parent_id,
            child_id,
        )
    }

    async fn build_dispatch_tool() -> (Arc<SchedulerTool>, ThreadId) {
        let db_path =
            std::env::temp_dir().join(format!("argus-dispatch-scheduler-{}.sqlite", Uuid::new_v4()));
        let pool = connect_path(&db_path).await.expect("create sqlite pool");
        migrate(&pool).await.expect("run migrations");
        let repo = Arc::new(ArgusSqlite::new(pool));
        let template_manager = Arc::new(TemplateManager::new(
            repo.clone() as Arc<dyn AgentRepository>,
            repo.clone(),
        ));

        template_manager
            .upsert(AgentRecord {
                id: AgentId::new(42),
                display_name: "Dispatch Test Agent".to_string(),
                description: "Dispatch worker".to_string(),
                version: "1.0.0".to_string(),
                provider_id: None,
                model_id: None,
                system_prompt: "You are a dispatch test agent.".to_string(),
                tool_names: vec![],
                max_tokens: None,
                temperature: None,
                thinking_config: Some(ThinkingConfig::enabled()),
                parent_agent_id: None,
                agent_type: AgentType::Standard,
            })
            .await
            .expect("create dispatch agent");

        let provider: Arc<dyn LlmProvider> = Arc::new(ImmediateProvider);
        let job_manager = Arc::new(JobManager::with_job_repository(
            template_manager.clone(),
            Arc::new(StaticProviderResolver { provider }),
            Arc::new(ToolManager::new()),
            repo.clone() as Arc<dyn JobRepository>,
        ));
        let workflow_manager = Arc::new(WorkflowManager::new(
            template_manager.clone(),
            repo.clone() as Arc<dyn WorkflowRepository>,
            repo as Arc<dyn JobRepository>,
            job_manager.clone(),
        ));

        (
            Arc::new(SchedulerTool::new(
                template_manager,
                job_manager,
                workflow_manager,
            )),
            ThreadId::new(),
        )
    }

    #[tokio::test]
    async fn definition_matches_scheduler_input_shape() {
        let tool = test_scheduler_tool();
        let definition = tool.definition();
        assert_eq!(definition.name, "scheduler");
        assert_eq!(definition.parameters["required"], serde_json::json!(["action"]));
        assert_eq!(definition.parameters["additionalProperties"], serde_json::json!(false));
        assert_eq!(
            definition.parameters["properties"]["action"]["enum"],
            serde_json::json!([
                "list_subagents",
                "dispatch_job",
                "get_job_result",
                "start_workflow",
                "get_workflow_progress"
            ])
        );
    }

    #[tokio::test]
    async fn list_subagents_requires_current_agent_id() {
        let tool = test_scheduler_tool();
        let result = tool
            .execute(
                serde_json::json!({
                    "action": "list_subagents",
                }),
                test_ctx(ThreadId::new()),
            )
            .await;

        assert!(matches!(
            result,
            Err(ToolError::ExecutionFailed { ref tool_name, ref reason })
                if tool_name == "scheduler" && reason.contains("current agent_id not available")
        ));
    }

    #[tokio::test]
    async fn list_subagents_returns_current_agent_subagents() {
        let (tool, parent_id, child_id) = build_list_subagents_tool().await;
        argus_agent::tool_context::set_current_agent_id(parent_id);

        let response = tool
            .execute(
                serde_json::json!({
                    "action": "list_subagents",
                }),
                test_ctx(ThreadId::new()),
            )
            .await
            .expect("tool should succeed");

        argus_agent::tool_context::clear_current_agent_id();

        let agents = response.as_array().expect("response should be an array");
        assert_eq!(agents.len(), 1);
        assert_eq!(agents[0]["agent_id"], serde_json::json!(child_id.inner()));
        assert_eq!(agents[0]["display_name"], serde_json::json!("Child Agent"));
    }

    #[tokio::test]
    async fn consume_true_claims_runtime_result_and_marks_job_consumed() {
        let tool = test_scheduler_tool();
        let thread_id = ThreadId::new();
        let result = completed_job("job-7");

        tool.job_manager
            .record_dispatched_job(thread_id, result.job_id.clone());
        tool.job_manager
            .record_completed_job_result(thread_id, result.clone());

        let (pipe_tx, _) = broadcast::channel(8);
        let (control_tx, mut control_rx) = mpsc::unbounded_channel();

        let reply_result = result.clone();
        tokio::spawn(async move {
            match control_rx.recv().await {
                Some(ThreadControlEvent::ClaimQueuedJobResult { job_id, reply_tx }) => {
                    assert_eq!(job_id, "job-7");
                    let _ = reply_tx.send(Some(reply_result));
                }
                other => panic!("unexpected control event: {other:?}"),
            }
        });

        let response = tool
            .execute(
                serde_json::json!({
                    "action": "get_job_result",
                    "job_id": "job-7",
                    "consume": true,
                }),
                Arc::new(ToolExecutionContext {
                    thread_id,
                    pipe_tx,
                    control_tx,
                }),
            )
            .await
            .expect("tool should succeed");

        assert_eq!(response["status"], serde_json::json!("consumed"));
        assert!(matches!(
            tool.job_manager
                .get_job_result_status(thread_id, "job-7", false),
            JobLookup::Pending
        ));
    }

    #[tokio::test]
    async fn get_job_result_does_not_leak_other_thread_results() {
        let tool = test_scheduler_tool();
        let owner_thread = ThreadId::new();
        let other_thread = ThreadId::new();
        let result = completed_job("job-11");

        tool.job_manager
            .record_dispatched_job(owner_thread, result.job_id.clone());
        tool.job_manager
            .record_completed_job_result(owner_thread, result);

        let response = tool
            .execute(
                serde_json::json!({
                    "action": "get_job_result",
                    "job_id": "job-11",
                }),
                test_ctx(other_thread),
            )
            .await
            .expect("tool should succeed");

        assert_eq!(response["status"], serde_json::json!("not_found"));
    }

    #[tokio::test]
    async fn start_workflow_returns_execution_id() {
        let (tool, thread_id) = build_start_workflow_tool().await;

        let response = tool
            .execute(
                serde_json::json!({
                    "action": "start_workflow",
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
    async fn get_workflow_progress_returns_grouped_counts() {
        let (tool, workflow_id, thread_id) = build_progress_tool().await;

        let response = tool
            .execute(
                serde_json::json!({
                    "action": "get_workflow_progress",
                    "workflow_execution_id": workflow_id.to_string(),
                }),
                test_ctx(thread_id),
            )
            .await
            .expect("tool should succeed");

        assert_eq!(response.pointer("/progress/total_nodes"), Some(&serde_json::json!(3)));
        assert_eq!(
            response.pointer("/progress/succeeded_nodes"),
            Some(&serde_json::json!(1))
        );
        assert_eq!(
            response.pointer("/progress/running_nodes"),
            Some(&serde_json::json!(1))
        );
        assert_eq!(
            response.pointer("/progress/pending_nodes"),
            Some(&serde_json::json!(1))
        );
    }

    #[tokio::test]
    async fn get_workflow_progress_does_not_leak_other_thread_progress() {
        let (tool, workflow_id, _thread_id) = build_progress_tool().await;

        let response = tool
            .execute(
                serde_json::json!({
                    "action": "get_workflow_progress",
                    "workflow_execution_id": workflow_id.to_string(),
                }),
                test_ctx(ThreadId::new()),
            )
            .await
            .expect("tool should succeed");

        assert_eq!(response["status"], serde_json::json!("not_found"));
    }

    #[tokio::test]
    async fn dispatch_job_returns_job_id_and_emits_dispatch_event() {
        let (tool, thread_id) = build_dispatch_tool().await;
        let (pipe_tx, mut pipe_rx) = broadcast::channel(8);
        let (control_tx, _control_rx) = mpsc::unbounded_channel();

        let response = tool
            .execute(
                serde_json::json!({
                    "action": "dispatch_job",
                    "prompt": "Run background work",
                    "agent_id": 42,
                }),
                Arc::new(ToolExecutionContext {
                    thread_id,
                    pipe_tx,
                    control_tx,
                }),
            )
            .await
            .expect("tool should dispatch");

        let event = pipe_rx.recv().await.expect("dispatch event");
        let job_id = response["job_id"]
            .as_str()
            .expect("job_id should be a string")
            .to_string();

        match event {
            ThreadEvent::JobDispatched {
                thread_id: event_thread_id,
                job_id: event_job_id,
                agent_id,
                prompt,
                ..
            } => {
                assert_eq!(event_thread_id, thread_id);
                assert_eq!(event_job_id, job_id);
                assert_eq!(agent_id, AgentId::new(42));
                assert_eq!(prompt, "Run background work");
            }
            other => panic!("unexpected event: {other:?}"),
        }

        assert_eq!(response["status"], serde_json::json!("dispatched"));
    }
}
