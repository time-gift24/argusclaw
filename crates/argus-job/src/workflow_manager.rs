//! WorkflowManager for persistent workflow instantiation and dispatch.

use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;

use argus_protocol::{AgentId, ThreadControlEvent, ThreadEvent, ThreadId};
use argus_repository::traits::{JobRepository, WorkflowRepository};
use argus_repository::types::{
    JobId, JobRecord, JobType, WorkflowId, WorkflowProgressRecord, WorkflowRecord, WorkflowStatus,
    WorkflowTemplateId, WorkflowTemplateNodeRecord, WorkflowTemplateRecord,
};
use argus_template::TemplateManager;
use serde::{Deserialize, Serialize};
use tokio::sync::{broadcast, mpsc};
use uuid::Uuid;

use crate::{JobError, JobManager};

/// A node appended at workflow instantiation time.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppendWorkflowNode {
    pub node_key: String,
    pub name: String,
    pub agent_id: AgentId,
    pub prompt: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context: Option<String>,
    #[serde(default)]
    pub depends_on_keys: Vec<String>,
}

/// Input for instantiating a workflow execution from a template.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstantiateWorkflowInput {
    pub template_id: WorkflowTemplateId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_version: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initiating_thread_id: Option<ThreadId>,
    #[serde(default)]
    pub extra_nodes: Vec<AppendWorkflowNode>,
}

/// Per-node progress summary for a workflow execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowExecutionNodeProgress {
    pub job_id: JobId,
    pub node_key: String,
    pub name: String,
    pub agent_id: AgentId,
    pub status: WorkflowStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub started_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub finished_at: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result_message: Option<String>,
}

/// Aggregated progress for a workflow execution.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowExecutionProgress {
    pub workflow_id: WorkflowId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_id: Option<WorkflowTemplateId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub template_version: Option<i64>,
    pub status: WorkflowStatus,
    pub total_nodes: i64,
    pub pending_nodes: i64,
    pub running_nodes: i64,
    pub succeeded_nodes: i64,
    pub failed_nodes: i64,
    pub cancelled_nodes: i64,
    #[serde(default)]
    pub nodes: Vec<WorkflowExecutionNodeProgress>,
}

/// A ready workflow job selected for dispatch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowDispatchJob {
    pub id: JobId,
    pub job_type: JobType,
    pub node_key: String,
    pub name: String,
    pub agent_id: AgentId,
}

impl From<&JobRecord> for WorkflowDispatchJob {
    fn from(job: &JobRecord) -> Self {
        Self {
            id: job.id.clone(),
            job_type: job.job_type,
            node_key: job.node_key.clone().unwrap_or_default(),
            name: job.name.clone(),
            agent_id: job.agent_id,
        }
    }
}

/// Manages persistent workflow execution.
pub struct WorkflowManager {
    template_manager: Arc<TemplateManager>,
    workflow_repo: Arc<dyn WorkflowRepository>,
    job_repo: Arc<dyn JobRepository>,
    job_manager: Arc<JobManager>,
}

impl WorkflowManager {
    /// Create a new workflow manager.
    pub fn new(
        template_manager: Arc<TemplateManager>,
        workflow_repo: Arc<dyn WorkflowRepository>,
        job_repo: Arc<dyn JobRepository>,
        job_manager: Arc<JobManager>,
    ) -> Self {
        Self {
            template_manager,
            workflow_repo,
            job_repo,
            job_manager,
        }
    }

    /// Instantiate a workflow execution from a template and append-only extra nodes.
    pub async fn instantiate_workflow(
        &self,
        input: InstantiateWorkflowInput,
    ) -> Result<WorkflowExecutionProgress, JobError> {
        let template = self
            .load_template(&input.template_id, input.template_version)
            .await?;
        let template_nodes = self
            .workflow_repo
            .list_workflow_template_nodes(&template.id, template.version)
            .await
            .map_err(|error| {
                JobError::Internal(format!(
                    "failed to load template nodes for {}@{}: {error}",
                    template.id, template.version
                ))
            })?;
        let node_specs = self
            .build_node_specs(template_nodes, input.extra_nodes)
            .await?;

        let execution_id = WorkflowId::new(format!("wf-{}", Uuid::new_v4().simple()));
        let execution = WorkflowRecord {
            id: execution_id.clone(),
            name: template.name.clone(),
            status: WorkflowStatus::Pending,
            template_id: Some(template.id.clone()),
            template_version: Some(template.version),
            initiating_thread_id: input.initiating_thread_id,
        };

        self.workflow_repo
            .create_workflow_execution(&execution)
            .await
            .map_err(|error| {
                JobError::Internal(format!(
                    "failed to create workflow execution {}: {error}",
                    execution_id
                ))
            })?;

        let jobs = Self::materialize_jobs(&execution_id, input.initiating_thread_id, &node_specs)?;
        if let Err(error) = self.persist_jobs(&execution_id, &jobs).await {
            self.cleanup_failed_instantiation(&execution_id).await;
            return Err(error);
        }

        self.dispatch_ready_jobs(&execution_id).await?;
        self.get_workflow_progress(&execution_id)
            .await?
            .ok_or_else(|| {
                JobError::Internal(format!(
                    "workflow execution {} disappeared after instantiation",
                    execution_id
                ))
            })
    }

    /// Return aggregated progress and node summaries for a workflow execution.
    pub async fn get_workflow_progress(
        &self,
        execution_id: &WorkflowId,
    ) -> Result<Option<WorkflowExecutionProgress>, JobError> {
        let progress = self
            .workflow_repo
            .get_workflow_progress(execution_id)
            .await
            .map_err(|error| {
                JobError::Internal(format!(
                    "failed to read progress for workflow {}: {error}",
                    execution_id
                ))
            })?;
        let Some(progress) = progress else {
            return Ok(None);
        };

        let status = self.recompute_workflow_status(execution_id).await?;
        let jobs = self
            .job_repo
            .list_by_group(execution_id.as_ref())
            .await
            .map_err(|error| {
                JobError::Internal(format!(
                    "failed to list jobs for workflow {}: {error}",
                    execution_id
                ))
            })?;

        let mut nodes = jobs
            .into_iter()
            .filter_map(|job| {
                Some(WorkflowExecutionNodeProgress {
                    job_id: job.id,
                    node_key: job.node_key?,
                    name: job.name,
                    agent_id: job.agent_id,
                    status: job.status,
                    started_at: job.started_at,
                    finished_at: job.finished_at,
                    result_message: job.result.map(|result| result.message),
                })
            })
            .collect::<Vec<_>>();
        nodes.sort_by(|left, right| left.node_key.cmp(&right.node_key));

        Ok(Some(WorkflowExecutionProgress {
            workflow_id: progress.workflow_id,
            template_id: progress.template_id,
            template_version: progress.template_version,
            status,
            total_nodes: progress.total_jobs,
            pending_nodes: progress.pending_jobs,
            running_nodes: progress.running_jobs,
            succeeded_nodes: progress.succeeded_jobs,
            failed_nodes: progress.failed_jobs,
            cancelled_nodes: progress.cancelled_jobs,
            nodes,
        }))
    }

    /// Dispatch currently ready jobs for one workflow execution.
    pub async fn dispatch_ready_jobs(
        &self,
        execution_id: &WorkflowId,
    ) -> Result<Vec<WorkflowDispatchJob>, JobError> {
        let execution = self
            .workflow_repo
            .get_workflow_execution(execution_id)
            .await
            .map_err(|error| {
                JobError::Internal(format!(
                    "failed to load workflow execution {}: {error}",
                    execution_id
                ))
            })?
            .ok_or_else(|| {
                JobError::ExecutionFailed(format!("workflow {} not found", execution_id))
            })?;
        let jobs = self
            .job_repo
            .list_by_group(execution_id.as_ref())
            .await
            .map_err(|error| {
                JobError::Internal(format!(
                    "failed to list jobs for workflow {}: {error}",
                    execution_id
                ))
            })?;
        let ready_jobs = Self::find_ready_jobs(&jobs);
        if ready_jobs.is_empty() {
            self.recompute_workflow_status(execution_id).await?;
            return Ok(Vec::new());
        }

        // Until workflow start tools pass live thread channels, use inert channels here.
        let (pipe_tx, _pipe_rx) = broadcast::channel::<ThreadEvent>(64);
        let (control_tx, _control_rx) = mpsc::unbounded_channel::<ThreadControlEvent>();
        let mut completion_rx = pipe_tx.subscribe();
        let workflow_repo = Arc::clone(&self.workflow_repo);
        let watched_execution_id = execution_id.clone();
        let mut remaining_jobs = ready_jobs
            .iter()
            .map(|job| job.id.to_string())
            .collect::<HashSet<_>>();
        let originating_thread_id = execution.initiating_thread_id.unwrap_or_else(ThreadId::new);

        tokio::spawn(async move {
            while !remaining_jobs.is_empty() {
                match completion_rx.recv().await {
                    Ok(ThreadEvent::JobResult { job_id, .. }) => {
                        if remaining_jobs.remove(&job_id)
                            && let Err(error) = Self::recompute_workflow_status_with_repo(
                                workflow_repo.as_ref(),
                                &watched_execution_id,
                            )
                            .await
                        {
                            tracing::warn!(
                                workflow_id = %watched_execution_id,
                                "failed to refresh workflow status after job completion: {error}"
                            );
                        }
                    }
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        for job in &ready_jobs {
            self.job_manager
                .spawn_persisted_job_executor(
                    originating_thread_id,
                    job.id.clone(),
                    pipe_tx.clone(),
                    control_tx.clone(),
                )
                .await?;
        }

        self.recompute_workflow_status(execution_id).await?;
        Ok(ready_jobs
            .into_iter()
            .map(WorkflowDispatchJob::from)
            .collect())
    }

    async fn load_template(
        &self,
        template_id: &WorkflowTemplateId,
        template_version: Option<i64>,
    ) -> Result<WorkflowTemplateRecord, JobError> {
        if let Some(version) = template_version {
            return self
                .workflow_repo
                .get_workflow_template(template_id, version)
                .await
                .map_err(|error| {
                    JobError::Internal(format!(
                        "failed to load workflow template {}@{}: {error}",
                        template_id, version
                    ))
                })?
                .ok_or_else(|| {
                    JobError::ExecutionFailed(format!(
                        "workflow template {}@{} was not found",
                        template_id, version
                    ))
                });
        }

        let templates = self
            .workflow_repo
            .list_workflow_templates()
            .await
            .map_err(|error| {
                JobError::Internal(format!(
                    "failed to list workflow templates for {}: {error}",
                    template_id
                ))
            })?;

        templates
            .into_iter()
            .filter(|template| template.id == *template_id)
            .max_by_key(|template| template.version)
            .ok_or_else(|| {
                JobError::ExecutionFailed(format!(
                    "workflow template {} was not found",
                    template_id
                ))
            })
    }

    async fn build_node_specs(
        &self,
        template_nodes: Vec<WorkflowTemplateNodeRecord>,
        extra_nodes: Vec<AppendWorkflowNode>,
    ) -> Result<Vec<NodeSpec>, JobError> {
        let mut nodes = Vec::with_capacity(template_nodes.len() + extra_nodes.len());

        for node in template_nodes {
            nodes.push(NodeSpec {
                node_key: node.node_key,
                name: node.name,
                agent_id: node.agent_id,
                prompt: node.prompt,
                context: node.context,
                depends_on_keys: node.depends_on_keys,
            });
        }

        for node in extra_nodes {
            nodes.push(NodeSpec {
                node_key: node.node_key,
                name: node.name,
                agent_id: node.agent_id,
                prompt: node.prompt,
                context: node.context,
                depends_on_keys: node.depends_on_keys,
            });
        }

        self.validate_agents(&nodes).await?;
        Self::topologically_sort_nodes(nodes)
    }

    async fn validate_agents(&self, nodes: &[NodeSpec]) -> Result<(), JobError> {
        let mut seen_agents = HashSet::new();
        for node in nodes {
            if !seen_agents.insert(node.agent_id) {
                continue;
            }

            let agent = self
                .template_manager
                .get(node.agent_id)
                .await
                .map_err(|error| {
                    JobError::Internal(format!(
                        "failed to validate agent {}: {error}",
                        node.agent_id.inner()
                    ))
                })?;
            if agent.is_none() {
                return Err(JobError::AgentNotFound(node.agent_id.inner()));
            }
        }

        Ok(())
    }

    fn materialize_jobs(
        execution_id: &WorkflowId,
        initiating_thread_id: Option<ThreadId>,
        nodes: &[NodeSpec],
    ) -> Result<Vec<JobRecord>, JobError> {
        let mut job_ids = HashMap::new();
        for node in nodes {
            job_ids.insert(
                node.node_key.clone(),
                JobId::new(format!("job-{}", Uuid::new_v4().simple())),
            );
        }

        nodes
            .iter()
            .map(|node| {
                let depends_on = node
                    .depends_on_keys
                    .iter()
                    .map(|key| {
                        job_ids.get(key).cloned().ok_or_else(|| {
                            JobError::ExecutionFailed(format!(
                                "workflow dependency {} was not materialized",
                                key
                            ))
                        })
                    })
                    .collect::<Result<Vec<_>, _>>()?;

                Ok(JobRecord {
                    id: job_ids.get(&node.node_key).cloned().ok_or_else(|| {
                        JobError::Internal(format!(
                            "missing materialized job id for node {}",
                            node.node_key
                        ))
                    })?,
                    job_type: JobType::Workflow,
                    name: node.name.clone(),
                    status: WorkflowStatus::Pending,
                    agent_id: node.agent_id,
                    context: node.context.clone(),
                    prompt: node.prompt.clone(),
                    thread_id: initiating_thread_id,
                    group_id: Some(execution_id.to_string()),
                    node_key: Some(node.node_key.clone()),
                    depends_on,
                    cron_expr: None,
                    scheduled_at: None,
                    started_at: None,
                    finished_at: None,
                    parent_job_id: None,
                    result: None,
                })
            })
            .collect()
    }

    async fn persist_jobs(
        &self,
        execution_id: &WorkflowId,
        jobs: &[JobRecord],
    ) -> Result<(), JobError> {
        for job in jobs {
            self.job_repo.create(job).await.map_err(|error| {
                JobError::Internal(format!(
                    "failed to create workflow job {} for {}: {error}",
                    job.id, execution_id
                ))
            })?;
        }

        Ok(())
    }

    async fn cleanup_failed_instantiation(&self, execution_id: &WorkflowId) {
        if let Err(error) = self
            .workflow_repo
            .delete_workflow_execution(execution_id)
            .await
        {
            tracing::error!(
                workflow_id = %execution_id,
                "failed to cleanup workflow after instantiation error: {error}"
            );
        }
    }

    async fn recompute_workflow_status(
        &self,
        execution_id: &WorkflowId,
    ) -> Result<WorkflowStatus, JobError> {
        Self::recompute_workflow_status_with_repo(self.workflow_repo.as_ref(), execution_id).await
    }

    async fn recompute_workflow_status_with_repo(
        workflow_repo: &dyn WorkflowRepository,
        execution_id: &WorkflowId,
    ) -> Result<WorkflowStatus, JobError> {
        let progress = workflow_repo
            .get_workflow_progress(execution_id)
            .await
            .map_err(|error| {
                JobError::Internal(format!(
                    "failed to read workflow {} progress: {error}",
                    execution_id
                ))
            })?
            .ok_or_else(|| {
                JobError::ExecutionFailed(format!("workflow {} not found", execution_id))
            })?;
        let target_status = Self::status_from_progress(&progress);
        if progress.status != target_status {
            workflow_repo
                .update_workflow_execution_status(execution_id, target_status)
                .await
                .map_err(|error| {
                    JobError::Internal(format!(
                        "failed to update workflow {} status to {}: {error}",
                        execution_id, target_status
                    ))
                })?;
        }

        Ok(target_status)
    }

    fn topologically_sort_nodes(nodes: Vec<NodeSpec>) -> Result<Vec<NodeSpec>, JobError> {
        let mut node_map = HashMap::new();
        for node in nodes {
            if node.node_key.trim().is_empty() {
                return Err(JobError::ExecutionFailed(
                    "workflow node key cannot be empty".to_string(),
                ));
            }
            if node.prompt.trim().is_empty() {
                return Err(JobError::ExecutionFailed(format!(
                    "workflow node {} prompt cannot be empty",
                    node.node_key
                )));
            }
            if node_map.insert(node.node_key.clone(), node).is_some() {
                return Err(JobError::ExecutionFailed(
                    "duplicate workflow node key".to_string(),
                ));
            }
        }

        let keys: HashSet<_> = node_map.keys().cloned().collect();
        let mut indegree: HashMap<String, usize> = HashMap::new();
        let mut outgoing: HashMap<String, Vec<String>> = HashMap::new();

        for (key, node) in &node_map {
            indegree.entry(key.clone()).or_insert(0);
            for dependency in &node.depends_on_keys {
                if !keys.contains(dependency) {
                    return Err(JobError::ExecutionFailed(format!(
                        "workflow dependency {} does not exist",
                        dependency
                    )));
                }

                *indegree.entry(key.clone()).or_insert(0) += 1;
                outgoing
                    .entry(dependency.clone())
                    .or_default()
                    .push(key.clone());
            }
        }

        let mut ready = indegree
            .iter()
            .filter(|(_, degree)| **degree == 0)
            .map(|(key, _)| key.clone())
            .collect::<VecDeque<_>>();
        let mut ordered = Vec::with_capacity(node_map.len());

        while let Some(key) = ready.pop_front() {
            let node = node_map.remove(&key).ok_or_else(|| {
                JobError::Internal(format!("workflow node {key} vanished during sort"))
            })?;
            ordered.push(node);

            for dependent in outgoing.remove(&key).unwrap_or_default() {
                let degree = indegree.get_mut(&dependent).ok_or_else(|| {
                    JobError::Internal(format!("workflow dependency state missing for {dependent}"))
                })?;
                *degree -= 1;
                if *degree == 0 {
                    ready.push_back(dependent);
                }
            }
        }

        if !node_map.is_empty() {
            return Err(JobError::ExecutionFailed(
                "workflow graph contains a cycle".to_string(),
            ));
        }

        Ok(ordered)
    }

    fn find_ready_jobs(jobs: &[JobRecord]) -> Vec<&JobRecord> {
        let completed = jobs
            .iter()
            .filter(|job| job.status == WorkflowStatus::Succeeded)
            .map(|job| job.id.clone())
            .collect::<HashSet<_>>();

        jobs.iter()
            .filter(|job| job.job_type == JobType::Workflow)
            .filter(|job| job.status == WorkflowStatus::Pending)
            .filter(|job| {
                job.depends_on
                    .iter()
                    .all(|dependency| completed.contains(dependency))
            })
            .collect()
    }

    fn status_from_progress(progress: &WorkflowProgressRecord) -> WorkflowStatus {
        if progress.failed_jobs > 0 {
            WorkflowStatus::Failed
        } else if progress.total_jobs > 0 && progress.cancelled_jobs == progress.total_jobs {
            WorkflowStatus::Cancelled
        } else if progress.total_jobs > 0 && progress.succeeded_jobs == progress.total_jobs {
            WorkflowStatus::Succeeded
        } else if progress.running_jobs > 0
            || progress.succeeded_jobs > 0
            || progress.cancelled_jobs > 0
        {
            WorkflowStatus::Running
        } else {
            WorkflowStatus::Pending
        }
    }
}

#[derive(Debug, Clone)]
struct NodeSpec {
    node_key: String,
    name: String,
    agent_id: AgentId,
    prompt: String,
    context: Option<String>,
    depends_on_keys: Vec<String>,
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use argus_protocol::llm::{
        CompletionRequest, CompletionResponse, FinishReason, LlmError, ToolCompletionRequest,
        ToolCompletionResponse,
    };
    use argus_protocol::{LlmProvider, ProviderId};
    use argus_repository::traits::{AgentRepository, JobRepository, WorkflowRepository};
    use argus_repository::types::{
        JobType, WorkflowTemplateId, WorkflowTemplateNodeRecord, WorkflowTemplateRecord,
    };
    use argus_repository::{ArgusSqlite, connect_path, migrate};
    use async_trait::async_trait;
    use rust_decimal::Decimal;
    use uuid::Uuid;

    use argus_tool::ToolManager;

    use super::*;

    #[derive(Debug)]
    struct ImmediateProvider;

    #[async_trait]
    impl LlmProvider for ImmediateProvider {
        fn model_name(&self) -> &str {
            "workflow-test-provider"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            unreachable!("complete is not used in workflow manager tests")
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
    impl argus_protocol::ProviderResolver for StaticProviderResolver {
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

    struct TestHarness {
        manager: WorkflowManager,
        repo: Arc<ArgusSqlite>,
    }

    async fn test_workflow_manager() -> TestHarness {
        let db_path =
            std::env::temp_dir().join(format!("argus-workflow-{}.sqlite", Uuid::new_v4()));
        let pool = connect_path(&db_path).await.expect("create sqlite pool");
        migrate(&pool).await.expect("run migrations");
        let repo = Arc::new(ArgusSqlite::new(pool));

        seed_test_agent(repo.as_ref(), 7, "Collector").await;
        seed_test_agent(repo.as_ref(), 8, "Summarizer").await;
        seed_test_agent(repo.as_ref(), 9, "Publisher").await;

        let template_manager = Arc::new(TemplateManager::new(
            repo.clone() as Arc<dyn AgentRepository>,
            repo.clone(),
        ));
        let provider: Arc<dyn LlmProvider> = Arc::new(ImmediateProvider);
        let job_manager = Arc::new(JobManager::with_job_repository(
            template_manager.clone(),
            Arc::new(StaticProviderResolver { provider }),
            Arc::new(ToolManager::new()),
            repo.clone() as Arc<dyn JobRepository>,
        ));

        TestHarness {
            manager: WorkflowManager::new(
                template_manager,
                repo.clone() as Arc<dyn WorkflowRepository>,
                repo.clone() as Arc<dyn JobRepository>,
                job_manager,
            ),
            repo,
        }
    }

    async fn seed_test_agent(repo: &ArgusSqlite, id: i64, display_name: &str) {
        let provider_id: i64 =
            sqlx::query_scalar("SELECT id FROM llm_providers ORDER BY id LIMIT 1")
                .fetch_one(repo.pool())
                .await
                .expect("default provider");

        sqlx::query(
            "INSERT INTO agents (id, display_name, description, version, provider_id, model_id, system_prompt, tool_names, max_tokens, temperature, thinking_config)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )
        .bind(id)
        .bind(display_name)
        .bind(format!("{display_name} agent"))
        .bind("1.0.0")
        .bind(provider_id)
        .bind(Option::<String>::None)
        .bind(format!("You are the {display_name} workflow agent."))
        .bind("[]")
        .bind(Option::<i64>::None)
        .bind(Option::<i64>::None)
        .bind(r#"{"type":"disabled","clear_thinking":false}"#)
        .execute(repo.pool())
        .await
        .expect("seed agent");
    }

    async fn seed_test_thread(repo: &ArgusSqlite, title: &str) -> ThreadId {
        let provider_id: i64 =
            sqlx::query_scalar("SELECT id FROM llm_providers ORDER BY id LIMIT 1")
                .fetch_one(repo.pool())
                .await
                .expect("default provider");

        let thread_id = ThreadId::new();
        sqlx::query(
            "INSERT INTO threads (id, provider_id, title, token_count, turn_count, session_id, template_id)
             VALUES (?1, ?2, ?3, 0, 0, NULL, NULL)",
        )
        .bind(thread_id.to_string())
        .bind(provider_id)
        .bind(title)
        .execute(repo.pool())
        .await
        .expect("seed thread");

        thread_id
    }

    async fn seed_template(
        repo: &ArgusSqlite,
        template_id: &str,
        version: i64,
        name: &str,
        nodes: Vec<WorkflowTemplateNodeRecord>,
    ) {
        repo.create_workflow_template(&WorkflowTemplateRecord {
            id: WorkflowTemplateId::new(template_id),
            name: name.to_string(),
            version,
            description: format!("{name} template"),
        })
        .await
        .expect("create template");

        for node in nodes {
            repo.create_workflow_template_node(&node)
                .await
                .expect("create template node");
        }
    }

    async fn wait_for_job_status(
        repo: &ArgusSqlite,
        execution_id: &WorkflowId,
        node_key: &str,
        expected: WorkflowStatus,
    ) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let jobs = JobRepository::list_by_group(repo, execution_id.as_ref())
                    .await
                    .expect("list workflow jobs");
                let Some(job) = jobs
                    .iter()
                    .find(|job| job.node_key.as_deref() == Some(node_key))
                else {
                    tokio::time::sleep(Duration::from_millis(10)).await;
                    continue;
                };

                if job.status == expected {
                    break;
                }

                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("job should reach expected status");
    }

    #[tokio::test]
    async fn instantiate_workflow_materializes_template_and_extra_nodes() {
        let harness = test_workflow_manager().await;
        let template_id = WorkflowTemplateId::new("tpl-1");

        seed_template(
            harness.repo.as_ref(),
            template_id.as_ref(),
            1,
            "Demo Workflow",
            vec![
                WorkflowTemplateNodeRecord {
                    template_id: template_id.clone(),
                    template_version: 1,
                    node_key: "collect".to_string(),
                    name: "Collect".to_string(),
                    agent_id: AgentId::new(7),
                    prompt: "Collect context".to_string(),
                    context: None,
                    depends_on_keys: vec![],
                },
                WorkflowTemplateNodeRecord {
                    template_id: template_id.clone(),
                    template_version: 1,
                    node_key: "publish".to_string(),
                    name: "Publish".to_string(),
                    agent_id: AgentId::new(9),
                    prompt: "Publish output".to_string(),
                    context: None,
                    depends_on_keys: vec!["collect".to_string()],
                },
                WorkflowTemplateNodeRecord {
                    template_id: template_id.clone(),
                    template_version: 1,
                    node_key: "notify".to_string(),
                    name: "Notify".to_string(),
                    agent_id: AgentId::new(8),
                    prompt: "Notify team".to_string(),
                    context: None,
                    depends_on_keys: vec!["publish".to_string()],
                },
            ],
        )
        .await;
        let initiating_thread_id = seed_test_thread(harness.repo.as_ref(), "workflow-start").await;

        let execution = harness
            .manager
            .instantiate_workflow(InstantiateWorkflowInput {
                template_id: template_id.clone(),
                template_version: Some(1),
                initiating_thread_id: Some(initiating_thread_id),
                extra_nodes: vec![AppendWorkflowNode {
                    node_key: "extra-review".to_string(),
                    name: "Extra Review".to_string(),
                    agent_id: AgentId::new(9),
                    prompt: "Review final output".to_string(),
                    context: None,
                    depends_on_keys: vec!["publish".to_string()],
                }],
            })
            .await
            .expect("instantiate workflow");

        assert_eq!(execution.total_nodes, 4);
        assert_eq!(execution.template_id, Some(template_id));
        assert_eq!(execution.template_version, Some(1));
        assert_eq!(execution.nodes.len(), 4);

        let jobs = harness
            .repo
            .list_by_group(execution.workflow_id.as_ref())
            .await
            .expect("list workflow jobs");
        assert_eq!(jobs.len(), 4);

        let publish_job = jobs
            .iter()
            .find(|job| job.node_key.as_deref() == Some("publish"))
            .expect("publish job should exist");
        let extra_review_job = jobs
            .iter()
            .find(|job| job.node_key.as_deref() == Some("extra-review"))
            .expect("extra review job should exist");

        assert_eq!(extra_review_job.depends_on, vec![publish_job.id.clone()]);
    }

    #[tokio::test]
    async fn instantiate_workflow_rejects_cycles() {
        let harness = test_workflow_manager().await;
        let template_id = WorkflowTemplateId::new("tpl-cycle");

        seed_template(
            harness.repo.as_ref(),
            template_id.as_ref(),
            1,
            "Cycle Workflow",
            vec![WorkflowTemplateNodeRecord {
                template_id: template_id.clone(),
                template_version: 1,
                node_key: "seed".to_string(),
                name: "Seed".to_string(),
                agent_id: AgentId::new(7),
                prompt: "Seed".to_string(),
                context: None,
                depends_on_keys: vec![],
            }],
        )
        .await;

        let error = harness
            .manager
            .instantiate_workflow(InstantiateWorkflowInput {
                template_id,
                template_version: Some(1),
                initiating_thread_id: None,
                extra_nodes: vec![
                    AppendWorkflowNode {
                        node_key: "a".to_string(),
                        name: "A".to_string(),
                        agent_id: AgentId::new(7),
                        prompt: "A".to_string(),
                        context: None,
                        depends_on_keys: vec!["b".to_string()],
                    },
                    AppendWorkflowNode {
                        node_key: "b".to_string(),
                        name: "B".to_string(),
                        agent_id: AgentId::new(7),
                        prompt: "B".to_string(),
                        context: None,
                        depends_on_keys: vec!["a".to_string()],
                    },
                ],
            })
            .await
            .expect_err("cycle must be rejected");

        assert!(matches!(error, JobError::ExecutionFailed(_)));
        assert!(error.to_string().contains("cycle"));
    }

    #[tokio::test]
    async fn dispatch_ready_jobs_after_upstream_success() {
        let harness = test_workflow_manager().await;
        let template_id = WorkflowTemplateId::new("tpl-dispatch");

        seed_template(
            harness.repo.as_ref(),
            template_id.as_ref(),
            1,
            "Dispatch Workflow",
            vec![
                WorkflowTemplateNodeRecord {
                    template_id: template_id.clone(),
                    template_version: 1,
                    node_key: "collect".to_string(),
                    name: "Collect".to_string(),
                    agent_id: AgentId::new(7),
                    prompt: "Collect context".to_string(),
                    context: None,
                    depends_on_keys: vec![],
                },
                WorkflowTemplateNodeRecord {
                    template_id: template_id.clone(),
                    template_version: 1,
                    node_key: "publish".to_string(),
                    name: "Publish".to_string(),
                    agent_id: AgentId::new(9),
                    prompt: "Publish output".to_string(),
                    context: None,
                    depends_on_keys: vec!["collect".to_string()],
                },
            ],
        )
        .await;
        let initiating_thread_id =
            seed_test_thread(harness.repo.as_ref(), "workflow-dispatch").await;

        let execution = harness
            .manager
            .instantiate_workflow(InstantiateWorkflowInput {
                template_id,
                template_version: Some(1),
                initiating_thread_id: Some(initiating_thread_id),
                extra_nodes: Vec::new(),
            })
            .await
            .expect("instantiate workflow");

        wait_for_job_status(
            harness.repo.as_ref(),
            &execution.workflow_id,
            "collect",
            WorkflowStatus::Succeeded,
        )
        .await;

        let ready_jobs = harness
            .manager
            .dispatch_ready_jobs(&execution.workflow_id)
            .await
            .expect("dispatch ready jobs");

        assert_eq!(ready_jobs.len(), 1);
        assert_eq!(ready_jobs[0].job_type, JobType::Workflow);
        assert_eq!(ready_jobs[0].node_key.as_str(), "publish");

        wait_for_job_status(
            harness.repo.as_ref(),
            &execution.workflow_id,
            "publish",
            WorkflowStatus::Succeeded,
        )
        .await;

        let progress = harness
            .manager
            .get_workflow_progress(&execution.workflow_id)
            .await
            .expect("read workflow progress")
            .expect("workflow should exist");
        assert_eq!(progress.status, WorkflowStatus::Succeeded);
        assert_eq!(progress.succeeded_nodes, 2);
    }
}
