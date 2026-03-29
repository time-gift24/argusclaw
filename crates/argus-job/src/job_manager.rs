//! JobManager for dispatching and managing background jobs.
//!
//! Each dispatched job is tracked through a ThreadPool-managed execution thread.
//! Results are sent back through the unified pipe as ThreadEvent::JobResult.

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};

use argus_agent::{TurnBuilder, TurnConfig, TurnOutput};
use argus_protocol::llm::{ChatMessage, Role};
use argus_protocol::{
    AgentId, ProviderResolver, ThreadControlEvent, ThreadEvent, ThreadId, ThreadJobResult,
    ThreadPoolRuntimeKind, ThreadPoolRuntimeRef, ThreadPoolSnapshot, ThreadPoolState,
};
use argus_repository::traits::JobRepository;
use argus_repository::types::{JobId, JobResult as PersistedJobResult, JobType, WorkflowStatus};
use argus_template::TemplateManager;
use argus_tool::ToolManager;
use chrono::Utc;
use futures_util::FutureExt;
use tokio::sync::{Mutex, broadcast, mpsc};

use crate::dispatch_tool::DispatchJobTool;
use crate::error::JobError;
use crate::get_job_result_tool::GetJobResultTool;
use crate::list_subagents_tool::ListSubagentsTool;
use crate::thread_pool::{ThreadPool, ThreadPoolPersistence};
use crate::types::ThreadPoolJobRequest;

#[derive(Debug, Clone)]
enum TrackedJobState {
    Pending,
    Completed(ThreadJobResult),
    Consumed(ThreadJobResult),
}

#[derive(Debug, Clone)]
struct TrackedJob {
    thread_id: ThreadId,
    state: TrackedJobState,
}

/// Result of looking up a background job for a specific thread.
#[derive(Debug, Clone)]
pub enum JobLookup {
    /// Job was never seen for this thread.
    NotFound,
    /// Job was dispatched but has not completed yet.
    Pending,
    /// Job completed and the result is still available for consumption.
    Completed(ThreadJobResult),
    /// Job result was already consumed proactively.
    Consumed(ThreadJobResult),
}

/// Manages job dispatch and lifecycle.
pub struct JobManager {
    template_manager: Arc<TemplateManager>,
    provider_resolver: Arc<dyn ProviderResolver>,
    tool_manager: Arc<ToolManager>,
    job_repo: Option<Arc<dyn JobRepository>>,
    tracked_jobs: Arc<StdMutex<HashMap<String, TrackedJob>>>,
}

impl fmt::Debug for JobManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobManager").finish()
    }
}

impl JobManager {
    #[cfg(test)]
    const JOB_RESULT_SUMMARY_CHAR_LIMIT: usize = 4000;

    /// Create a new JobManager.
    pub fn new(
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        tool_manager: Arc<ToolManager>,
        compactor_manager: Arc<CompactorManager>,
        trace_dir: PathBuf,
    ) -> Self {
        Self::build(template_manager, provider_resolver, tool_manager, None)
    }

    /// Create a new JobManager with a repository-backed execution path.
    pub fn with_job_repository(
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        tool_manager: Arc<ToolManager>,
        job_repo: Arc<dyn JobRepository>,
    ) -> Self {
        Self::build(
            template_manager,
            provider_resolver,
            tool_manager,
            Some(job_repo),
        )
    }

    fn build(
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        tool_manager: Arc<ToolManager>,
        job_repo: Option<Arc<dyn JobRepository>>,
    ) -> Self {
        Self {
            template_manager,
            provider_resolver,
            tool_manager,
            job_repo,
            tracked_jobs: Arc::new(StdMutex::new(HashMap::new())),
        }
    }

    /// Create a new JobManager wired with repository-backed persistence.
    pub fn new_with_repositories(
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        tool_manager: Arc<ToolManager>,
        compactor_manager: Arc<CompactorManager>,
        trace_dir: PathBuf,
        job_repository: Arc<dyn JobRepository>,
        thread_repository: Arc<dyn ThreadRepository>,
        provider_repository: Arc<dyn LlmProviderRepository>,
    ) -> Self {
        Self::new_with_persistence(
            template_manager,
            provider_resolver,
            tool_manager,
            compactor_manager,
            trace_dir,
            Some(ThreadPoolPersistence::new(
                job_repository,
                thread_repository,
                provider_repository,
            )),
        )
    }

    /// Create a DispatchJobTool for this manager.
    pub fn create_dispatch_tool(self: Arc<Self>) -> DispatchJobTool {
        DispatchJobTool::new(self)
    }

    /// Create a ListSubagentsTool for this manager.
    pub fn create_list_subagents_tool(self: Arc<Self>) -> ListSubagentsTool {
        ListSubagentsTool::new(Arc::clone(&self.template_manager))
    }

    /// Create a GetJobResultTool for this manager.
    pub fn create_get_job_result_tool(self: Arc<Self>) -> GetJobResultTool {
        GetJobResultTool::new(self)
    }

    /// Get the currently bound execution thread for a job, if any.
    pub fn thread_binding(&self, job_id: &str) -> Option<ThreadId> {
        self.thread_pool.get_thread_binding(job_id)
    }

    /// Return the shared unified thread pool.
    pub fn thread_pool(&self) -> Arc<ThreadPool> {
        Arc::clone(&self.thread_pool)
    }

    /// Collect a point-in-time thread-pool snapshot.
    pub fn thread_pool_snapshot(&self) -> ThreadPoolSnapshot {
        self.thread_pool.collect_metrics()
    }

    /// Collect the authoritative thread-pool state.
    pub fn thread_pool_state(&self) -> ThreadPoolState {
        self.thread_pool.collect_state()
    }

    /// Record that a job was dispatched for a thread.
    pub fn record_dispatched_job(&self, thread_id: ThreadId, job_id: String) {
        Self::record_dispatched_job_in_store(&self.tracked_jobs, thread_id, job_id);
    }

    /// Record the completed result for a job.
    pub fn record_completed_job_result(&self, thread_id: ThreadId, result: ThreadJobResult) {
        Self::record_completed_job_result_in_store(&self.tracked_jobs, thread_id, result);
    }

    /// Get the current tracked status for a job scoped to its originating thread.
    pub fn get_job_result_status(
        &self,
        thread_id: ThreadId,
        job_id: &str,
        consume: bool,
    ) -> JobLookup {
        let mut tracked_jobs = self
            .tracked_jobs
            .lock()
            .expect("job tracking mutex poisoned");
        let Some(tracked_job) = tracked_jobs.get_mut(job_id) else {
            return JobLookup::NotFound;
        };

        if tracked_job.thread_id != thread_id {
            return JobLookup::NotFound;
        }

        match &tracked_job.state {
            TrackedJobState::Pending => JobLookup::Pending,
            TrackedJobState::Completed(result) => {
                let result = result.clone();
                if consume {
                    tracked_job.state = TrackedJobState::Consumed(result.clone());
                }
                JobLookup::Completed(result)
            }
            TrackedJobState::Consumed(result) => JobLookup::Consumed(result.clone()),
        }
    }

    /// Dispatch a background job through the thread pool.
    #[allow(clippy::too_many_arguments)]
    pub async fn dispatch_job(
        &self,
        originating_thread_id: ThreadId,
        job_id: String,
        agent_id: AgentId,
        prompt: String,
        context: Option<serde_json::Value>,
        pipe_tx: broadcast::Sender<ThreadEvent>,
        control_tx: mpsc::UnboundedSender<ThreadControlEvent>,
    ) -> Result<(), JobError> {
        if let Some(job_repo) = &self.job_repo {
            match job_repo.get(&JobId::new(job_id.clone())).await {
                Ok(Some(job)) if job.job_type == JobType::Workflow => {
                    return self
                        .spawn_persisted_job_executor(
                            originating_thread_id,
                            job.id,
                            pipe_tx,
                            control_tx,
                        )
                        .await;
                }
                Ok(_) => {}
                Err(error) => {
                    tracing::warn!(
                        job_id,
                        "failed to inspect persisted job before ad hoc execution: {error}"
                    );
                }
            }
        }

        if prompt.trim().is_empty() {
            return Err(JobError::ExecutionFailed(
                "prompt cannot be empty".to_string(),
            ));
        }

        let request = ThreadPoolJobRequest {
            originating_thread_id,
            job_id: job_id.clone(),
            agent_id,
            prompt,
            context,
        };

        let execution_thread_id = self.thread_pool.enqueue_job(request.clone()).await?;
        self.record_dispatched_job(originating_thread_id, job_id.clone());
        let _ = pipe_tx.send(ThreadEvent::ThreadBoundToJob {
            job_id: job_id.clone(),
            thread_id: execution_thread_id,
        });
        let _ = pipe_tx.send(ThreadEvent::ThreadPoolQueued {
            runtime: ThreadPoolRuntimeRef {
                thread_id: execution_thread_id,
                kind: ThreadPoolRuntimeKind::Job,
                session_id: None,
                job_id: Some(job_id.clone()),
            },
        });
        let _ = pipe_tx.send(ThreadEvent::ThreadPoolMetricsUpdated {
            snapshot: self.thread_pool.collect_metrics(),
        });

        let thread_pool = Arc::clone(&self.thread_pool);
        let tracked_jobs = Arc::clone(&self.tracked_jobs);
        let pipe_tx_clone = pipe_tx.clone();
        let control_tx_clone = control_tx.clone();

        tokio::spawn(async move {
            let result = thread_pool
                .execute_job(
                    request,
                    execution_thread_id,
                    pipe_tx_clone.clone(),
                    control_tx_clone.clone(),
                )
                .await;

            Self::forward_job_result_to_runtime(&control_tx_clone, result.clone());
            Self::record_completed_job_result_in_store(
                &tracked_jobs,
                originating_thread_id,
                result.clone(),
            );
            Self::broadcast_job_result(&pipe_tx_clone, originating_thread_id, result);
        });

        Ok(())
    }

    /// Spawn a repository-backed workflow job executor.
    pub async fn spawn_persisted_job_executor(
        &self,
        originating_thread_id: ThreadId,
        job_id: JobId,
        pipe_tx: broadcast::Sender<ThreadEvent>,
        control_tx: mpsc::UnboundedSender<ThreadControlEvent>,
    ) -> Result<(), JobError> {
        let job_repo = self
            .job_repo
            .clone()
            .ok_or_else(|| JobError::Internal("job repository not configured".to_string()))?;
        let job = job_repo
            .get(&job_id)
            .await
            .map_err(|error| JobError::Internal(format!("failed to load job {job_id}: {error}")))?
            .ok_or_else(|| JobError::JobNotFound(job_id.to_string()))?;

        if job.prompt.trim().is_empty() {
            return Err(JobError::ExecutionFailed(
                "prompt cannot be empty".to_string(),
            ));
        }

        let started_at = Utc::now().to_rfc3339();
        job_repo
            .update_status(&job_id, WorkflowStatus::Running, Some(&started_at), None)
            .await
            .map_err(|error| {
                JobError::Internal(format!(
                    "failed to mark job {} running: {}",
                    job_id, error
                ))
            })?;

        self.record_dispatched_job(originating_thread_id, job_id.to_string());

        let template_manager = Arc::clone(&self.template_manager);
        let provider_resolver = Arc::clone(&self.provider_resolver);
        let tool_manager = Arc::clone(&self.tool_manager);
        let tracked_jobs = Arc::clone(&self.tracked_jobs);
        let pipe_tx_clone = pipe_tx.clone();
        let control_tx_clone = control_tx.clone();
        let job_repo_clone = Arc::clone(&job_repo);
        let job_id_string = job_id.to_string();
        let agent_id = job.agent_id;
        let prompt = job.prompt;

        tokio::spawn(async move {
            let fallback_job_id = job_id_string.clone();
            let fallback_display_name = format!("Agent {}", agent_id.inner());
            let result = AssertUnwindSafe(Self::execute_job(
                template_manager,
                provider_resolver,
                tool_manager,
                originating_thread_id,
                job_id_string,
                agent_id,
                prompt,
                pipe_tx_clone.clone(),
                control_tx_clone.clone(),
            ))
            .catch_unwind()
            .await;

            let result = match result {
                Ok(result) => result,
                Err(payload) => Self::failure_result(
                    fallback_job_id,
                    agent_id,
                    fallback_display_name,
                    String::new(),
                    Self::panic_message(payload),
                ),
            };

            let result = match Self::persist_job_completion(&job_repo_clone, &job_id, &started_at, &result)
                .await
            {
                Ok(()) => Some(result),
                Err(error) => {
                    let failure_result = Self::failure_result(
                        job_id.to_string(),
                        result.agent_id,
                        result.agent_display_name.clone(),
                        result.agent_description.clone(),
                        format!("failed to persist job completion: {error}"),
                    );

                    match Self::persist_job_completion(
                        &job_repo_clone,
                        &job_id,
                        &started_at,
                        &failure_result,
                    )
                    .await
                    {
                        Ok(()) => Some(failure_result),
                        Err(recovery_error) => {
                            tracing::error!(
                                job_id = %job_id,
                                "failed to persist fallback job failure: {recovery_error}"
                            );
                            None
                        }
                    }
                }
            };

            let Some(result) = result else {
                return;
            };

            Self::forward_job_result_to_runtime(&control_tx_clone, result.clone());
            Self::record_completed_job_result_in_store(
                &tracked_jobs,
                originating_thread_id,
                result.clone(),
            );
            Self::broadcast_job_result(&pipe_tx_clone, originating_thread_id, result);
        });

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_job(
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        tool_manager: Arc<ToolManager>,
        originating_thread_id: ThreadId,
        job_id: String,
        agent_id: AgentId,
        prompt: String,
        pipe_tx: broadcast::Sender<ThreadEvent>,
        control_tx: mpsc::UnboundedSender<ThreadControlEvent>,
    ) -> ThreadJobResult {
        let thread_id = format!("job-{}", job_id);
        let default_display_name = format!("Agent {}", agent_id.inner());

        let agent_record = match template_manager.get(agent_id).await {
            Ok(Some(record)) => record,
            Ok(None) => {
                return Self::failure_result(
                    job_id,
                    agent_id,
                    default_display_name,
                    String::new(),
                    format!("agent {} not found", agent_id.inner()),
                );
            }
            Err(e) => {
                return Self::failure_result(
                    job_id,
                    agent_id,
                    default_display_name,
                    String::new(),
                    format!("failed to load agent: {}", e),
                );
            }
        };
        let agent_display_name = agent_record.display_name.clone();
        let agent_description = agent_record.description.clone();

        let provider = match agent_record.provider_id {
            Some(pid) => match provider_resolver.resolve(pid).await {
                Ok(p) => p,
                Err(e) => {
                    return Self::failure_result(
                        job_id,
                        agent_id,
                        agent_display_name.clone(),
                        agent_description.clone(),
                        format!("failed to resolve provider: {}", e),
                    );
                }
            },
            None => match provider_resolver.default_provider().await {
                Ok(p) => p,
                Err(e) => {
                    return Self::failure_result(
                        job_id,
                        agent_id,
                        agent_display_name.clone(),
                        agent_description.clone(),
                        format!("no provider configured: {}", e),
                    );
                }
            },
        };

        let enabled_tool_names: HashSet<_> = agent_record.tool_names.iter().collect();
        let tools: Vec<Arc<dyn NamedTool>> = tool_manager
            .list_ids()
            .iter()
            .filter(|name| enabled_tool_names.contains(*name))
            .filter_map(|name| tool_manager.get(name))
            .collect();

        let (stream_tx, _stream_rx) = broadcast::channel(256);

        let turn = match TurnBuilder::default()
            .turn_number(1)
            .thread_id(thread_id)
            .messages(vec![ChatMessage::user(&prompt)])
            .provider(provider)
            .tools(tools)
            .hooks(Vec::new())
            .config(TurnConfig::new())
            .agent_record(Arc::new(agent_record))
            .stream_tx(stream_tx)
            .thread_event_tx(pipe_tx)
            .originating_thread_id(originating_thread_id)
            .control_tx(control_tx)
            .mailbox(Arc::new(Mutex::new(ThreadMailbox::default())))
            .build()
        {
            Ok(turn) => turn,
            Err(e) => {
                return Self::failure_result(
                    job_id,
                    agent_id,
                    agent_display_name.clone(),
                    agent_description.clone(),
                    format!("failed to build turn: {}", e),
                );
            }
        };

        match turn.execute().await {
            Ok(output) => ThreadJobResult {
                job_id,
                success: true,
                message: Self::summarize_output(&output),
                token_usage: Some(output.token_usage),
                agent_id,
                agent_display_name,
                agent_description,
            },
            Err(e) => Self::failure_result(
                job_id,
                agent_id,
                agent_display_name,
                agent_description,
                e.to_string(),
            ),
        }
    }

    /// Summarize turn output into a brief result message.
    #[cfg(test)]
    fn summarize_output(output: &TurnOutput) -> String {
        for msg in output.messages.iter().rev() {
            if let ChatMessage {
                role: Role::Assistant,
                content,
                ..
            } = msg
                && !content.is_empty()
            {
                return Self::truncate_summary(content);
            }
        }
        format!("job completed, {} messages in turn", output.messages.len())
    }

    #[cfg(test)]
    fn truncate_summary(content: &str) -> String {
        let mut chars = content.chars();
        let summary: String = chars
            .by_ref()
            .take(Self::JOB_RESULT_SUMMARY_CHAR_LIMIT)
            .collect();
        if chars.next().is_some() {
            format!("{summary}...")
        } else {
            content.to_string()
        }
    }

    fn failure_result(
        job_id: String,
        agent_id: AgentId,
        agent_display_name: String,
        agent_description: String,
        message: String,
    ) -> ThreadJobResult {
        ThreadJobResult {
            job_id,
            success: false,
            message,
            token_usage: None,
            agent_id,
            agent_display_name,
            agent_description,
        }
    }

    fn panic_message(payload: Box<dyn Any + Send>) -> String {
        let payload = payload.as_ref();
        let detail = payload
            .downcast_ref::<&'static str>()
            .map(|msg| (*msg).to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic payload".to_string());
        format!("job executor panicked: {detail}")
    }

    async fn persist_job_completion(
        job_repo: &Arc<dyn JobRepository>,
        job_id: &JobId,
        started_at: &str,
        result: &ThreadJobResult,
    ) -> Result<(), JobError> {
        let persisted_result = PersistedJobResult {
            success: result.success,
            message: result.message.clone(),
            token_usage: result.token_usage.clone(),
            agent_id: result.agent_id,
            agent_display_name: result.agent_display_name.clone(),
            agent_description: result.agent_description.clone(),
        };
        let final_status = if result.success {
            WorkflowStatus::Succeeded
        } else {
            WorkflowStatus::Failed
        };
        let finished_at = Utc::now().to_rfc3339();

        job_repo
            .update_result(job_id, &persisted_result)
            .await
            .map_err(|error| {
                JobError::Internal(format!(
                    "failed to persist result for job {}: {}",
                    job_id, error
                ))
            })?;

        job_repo
            .update_status(job_id, final_status, Some(started_at), Some(&finished_at))
            .await
            .map_err(|error| {
                JobError::Internal(format!(
                    "failed to persist terminal status for job {}: {}",
                    job_id, error
                ))
            })?;

        Ok(())
    }

    fn record_dispatched_job_in_store(
        tracked_jobs: &Arc<StdMutex<HashMap<String, TrackedJob>>>,
        thread_id: ThreadId,
        job_id: String,
    ) {
        tracked_jobs
            .lock()
            .expect("job tracking mutex poisoned")
            .insert(
                job_id,
                TrackedJob {
                    thread_id,
                    state: TrackedJobState::Pending,
                },
            );
    }

    fn record_completed_job_result_in_store(
        tracked_jobs: &Arc<StdMutex<HashMap<String, TrackedJob>>>,
        thread_id: ThreadId,
        result: ThreadJobResult,
    ) {
        tracked_jobs
            .lock()
            .expect("job tracking mutex poisoned")
            .insert(
                result.job_id.clone(),
                TrackedJob {
                    thread_id,
                    state: TrackedJobState::Completed(result),
                },
            );
    }

    fn forward_job_result_to_runtime(
        control_tx: &mpsc::UnboundedSender<ThreadControlEvent>,
        result: ThreadJobResult,
    ) {
        let _ = control_tx.send(ThreadControlEvent::JobResult(result));
    }

    fn broadcast_job_result(
        pipe_tx: &broadcast::Sender<ThreadEvent>,
        originating_thread_id: ThreadId,
        result: ThreadJobResult,
    ) {
        let _ = pipe_tx.send(ThreadEvent::JobResult {
            thread_id: originating_thread_id,
            job_id: result.job_id,
            success: result.success,
            message: result.message,
            token_usage: result.token_usage,
            agent_id: result.agent_id,
            agent_display_name: result.agent_display_name,
            agent_description: result.agent_description,
        });
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use std::time::Duration;

    use argus_protocol::llm::{
        CompletionRequest, CompletionResponse, FinishReason, LlmError, ToolCompletionRequest,
        ToolCompletionResponse,
    };
    use argus_protocol::{LlmProvider, ProviderId, ThreadEvent, ThreadId};
    use argus_repository::traits::{AgentRepository, JobRepository, WorkflowRepository};
    use argus_repository::types::{JobId, JobRecord, JobType, WorkflowId, WorkflowRecord};
    use argus_repository::{ArgusSqlite, DbError, connect_path, migrate};
    use argus_template::TemplateManager;
    use async_trait::async_trait;
    use rust_decimal::Decimal;
    use sqlx::SqlitePool;
    use tokio::sync::{broadcast, mpsc, oneshot};
    use uuid::Uuid;

    use argus_protocol::TokenUsage;
    use argus_tool::ToolManager;
    use tokio::time::{Duration, timeout};

    use super::*;

    #[derive(Debug)]
    struct DummyProviderResolver;

    #[async_trait]
    impl ProviderResolver for DummyProviderResolver {
        async fn resolve(&self, _id: ProviderId) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
            unreachable!("resolver should not be called in tracking tests");
        }

        async fn default_provider(&self) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
            unreachable!("resolver should not be called in tracking tests");
        }

        async fn resolve_with_model(
            &self,
            _id: ProviderId,
            _model: &str,
        ) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
            unreachable!("resolver should not be called in tracking tests");
        }
    }

    #[derive(Debug)]
    struct ControlledProvider {
        response: String,
        release_rx: tokio::sync::Mutex<Option<oneshot::Receiver<()>>>,
    }

    impl ControlledProvider {
        fn new(response: impl Into<String>, release_rx: oneshot::Receiver<()>) -> Self {
            Self {
                response: response.into(),
                release_rx: tokio::sync::Mutex::new(Some(release_rx)),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for ControlledProvider {
        fn model_name(&self) -> &str {
            "controlled"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            unreachable!("complete is not used in job manager tests")
        }

        async fn complete_with_tools(
            &self,
            _request: ToolCompletionRequest,
        ) -> Result<ToolCompletionResponse, LlmError> {
            if let Some(release_rx) = self.release_rx.lock().await.take() {
                let _ = release_rx.await;
            }

            Ok(ToolCompletionResponse {
                content: Some(self.response.clone()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: 5,
                output_tokens: 7,
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

    struct FailFirstResultWriteRepo {
        inner: Arc<ArgusSqlite>,
        fail_next_result_write: AtomicBool,
    }

    impl FailFirstResultWriteRepo {
        fn new(inner: Arc<ArgusSqlite>) -> Self {
            Self {
                inner,
                fail_next_result_write: AtomicBool::new(true),
            }
        }
    }

    #[async_trait]
    impl JobRepository for FailFirstResultWriteRepo {
        async fn create(&self, job: &JobRecord) -> Result<(), DbError> {
            JobRepository::create(self.inner.as_ref(), job).await
        }

        async fn get(&self, id: &JobId) -> Result<Option<JobRecord>, DbError> {
            JobRepository::get(self.inner.as_ref(), id).await
        }

        async fn update_status(
            &self,
            id: &JobId,
            status: WorkflowStatus,
            started_at: Option<&str>,
            finished_at: Option<&str>,
        ) -> Result<(), DbError> {
            JobRepository::update_status(self.inner.as_ref(), id, status, started_at, finished_at)
                .await
        }

        async fn update_result(
            &self,
            id: &JobId,
            result: &PersistedJobResult,
        ) -> Result<(), DbError> {
            if self.fail_next_result_write.swap(false, Ordering::SeqCst) {
                return Err(DbError::QueryFailed {
                    reason: format!("injected result persistence failure for job {}", id),
                });
            }

            JobRepository::update_result(self.inner.as_ref(), id, result).await
        }

        async fn update_thread_id(&self, id: &JobId, thread_id: &ThreadId) -> Result<(), DbError> {
            JobRepository::update_thread_id(self.inner.as_ref(), id, thread_id).await
        }

        async fn find_ready_jobs(&self, limit: usize) -> Result<Vec<JobRecord>, DbError> {
            JobRepository::find_ready_jobs(self.inner.as_ref(), limit).await
        }

        async fn find_due_cron_jobs(&self, now: &str) -> Result<Vec<JobRecord>, DbError> {
            JobRepository::find_due_cron_jobs(self.inner.as_ref(), now).await
        }

        async fn update_scheduled_at(&self, id: &JobId, next: &str) -> Result<(), DbError> {
            JobRepository::update_scheduled_at(self.inner.as_ref(), id, next).await
        }

        async fn list_by_group(&self, group_id: &str) -> Result<Vec<JobRecord>, DbError> {
            JobRepository::list_by_group(self.inner.as_ref(), group_id).await
        }

        async fn delete(&self, id: &JobId) -> Result<bool, DbError> {
            JobRepository::delete(self.inner.as_ref(), id).await
        }
    }

    fn test_job_manager() -> JobManager {
        let pool = SqlitePool::connect_lazy("sqlite::memory:")
            .expect("lazy sqlite pool should build for tests");
        let sqlite = Arc::new(ArgusSqlite::new(pool));
        JobManager::new(
            Arc::new(TemplateManager::new(
                sqlite.clone() as Arc<dyn AgentRepository>,
                sqlite.clone(),
            )),
            Arc::new(DummyProviderResolver),
            Arc::new(ToolManager::new()),
            Arc::new(CompactorManager::with_defaults()),
            std::env::temp_dir().join("argus-job-tests"),
        )
    }

    async fn test_persistent_job_manager_without_default_provider() -> JobManager {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("sqlite memory pool should connect");
        migrate(&pool).await.expect("migration should succeed");
        let sqlite = Arc::new(ArgusSqlite::new(pool));

        let providers = LlmProviderRepository::list_providers(sqlite.as_ref())
            .await
            .expect("provider list should load");
        for provider in providers {
            LlmProviderRepository::delete_provider(sqlite.as_ref(), &provider.id)
                .await
                .expect("provider should delete");
        }

        JobManager::new_with_repositories(
            Arc::new(TemplateManager::new(
                sqlite.clone() as Arc<dyn AgentRepository>,
                sqlite.clone(),
            )),
            Arc::new(DummyProviderResolver),
            Arc::new(ToolManager::new()),
            Arc::new(CompactorManager::with_defaults()),
            std::env::temp_dir().join("argus-job-tests"),
            sqlite.clone() as Arc<dyn JobRepository>,
            sqlite.clone() as Arc<dyn ThreadRepository>,
            sqlite as Arc<dyn LlmProviderRepository>,
        )
    }

    async fn seeded_repo() -> Arc<ArgusSqlite> {
        let db_path = std::env::temp_dir().join(format!("argus-job-{}.sqlite", Uuid::new_v4()));
        let pool = connect_path(&db_path).await.expect("create sqlite pool");
        migrate(&pool).await.expect("run migrations");
        Arc::new(ArgusSqlite::new(pool))
    }

    async fn seed_test_agent(repo: &ArgusSqlite) -> AgentId {
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
        .bind("Workflow Test Agent")
        .bind("Test agent")
        .bind("1.0.0")
        .bind(provider_id)
        .bind(Option::<String>::None)
        .bind("You are a workflow test agent.")
        .bind("[]")
        .bind(Option::<i64>::None)
        .bind(Option::<i64>::None)
        .bind(r#"{"type":"disabled","clear_thinking":false}"#)
        .execute(repo.pool())
        .await
        .expect("seed agent");

        AgentId::new(7)
    }

    async fn seed_persisted_workflow_job(repo: &ArgusSqlite, job_id: &JobId, agent_id: AgentId) {
        let workflow_id = WorkflowId::new("wf-persisted");
        repo.create_workflow_execution(&WorkflowRecord {
            id: workflow_id.clone(),
            name: "Persisted Workflow".to_string(),
            status: WorkflowStatus::Pending,
            template_id: None,
            template_version: None,
            initiating_thread_id: None,
        })
        .await
        .expect("create workflow execution");

        repo.create(&JobRecord {
            id: job_id.clone(),
            job_type: JobType::Workflow,
            name: "Persisted Job".to_string(),
            status: WorkflowStatus::Pending,
            agent_id,
            context: None,
            prompt: "Summarize the repository".to_string(),
            thread_id: None,
            group_id: Some(workflow_id.to_string()),
            node_key: Some("summarize".to_string()),
            depends_on: Vec::new(),
            cron_expr: None,
            scheduled_at: None,
            started_at: None,
            finished_at: None,
            parent_job_id: None,
            result: None,
        })
        .await
        .expect("create persisted workflow job");
    }

    async fn wait_for_running_status(repo: &ArgusSqlite, job_id: &JobId) {
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                let job = JobRepository::get(repo, job_id)
                    .await
                    .expect("load job")
                    .expect("job should exist");
                if job.status == WorkflowStatus::Running {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("job should reach running state");
    }

    fn assistant_output(content: &str) -> TurnOutput {
        TurnOutput {
            messages: vec![ChatMessage::assistant(content)],
            token_usage: TokenUsage::default(),
        }
    }

    #[test]
    fn summarize_output_handles_unicode_boundaries() {
        let content = format!("{}数{}", "a".repeat(498), "b".repeat(5000));

        let summary = JobManager::summarize_output(&assistant_output(&content));

        assert!(summary.ends_with("..."));
        assert_eq!(
            summary.chars().count(),
            JobManager::JOB_RESULT_SUMMARY_CHAR_LIMIT + 3
        );
        assert!(summary.contains('数'));
    }

    #[test]
    fn summarize_output_keeps_reports_longer_than_legacy_limit() {
        let content = "x".repeat(800);

        let summary = JobManager::summarize_output(&assistant_output(&content));

        assert_eq!(summary, content);
    }

    #[tokio::test]
    async fn tracked_job_status_moves_from_pending_to_completed_to_consumed() {
        let manager = test_job_manager();
        let thread_id = ThreadId::new();
        let result = ThreadJobResult {
            job_id: "job-42".to_string(),
            success: true,
            message: "all done".to_string(),
            token_usage: None,
            agent_id: AgentId::new(9),
            agent_display_name: "Researcher".to_string(),
            agent_description: "Looks things up".to_string(),
        };

        manager.record_dispatched_job(thread_id, result.job_id.clone());
        assert!(matches!(
            manager.get_job_result_status(thread_id, &result.job_id, false),
            JobLookup::Pending
        ));

        manager.record_completed_job_result(thread_id, result.clone());
        assert!(matches!(
            manager.get_job_result_status(thread_id, &result.job_id, false),
            JobLookup::Completed(found) if found.job_id == result.job_id
        ));

        assert!(matches!(
            manager.get_job_result_status(thread_id, &result.job_id, true),
            JobLookup::Completed(found) if found.job_id == result.job_id
        ));

        assert!(matches!(
            manager.get_job_result_status(thread_id, &result.job_id, false),
            JobLookup::Consumed(found) if found.job_id == result.job_id
        ));
    }

    #[tokio::test]
    async fn persisted_workflow_job_updates_status_and_result() {
        let repo = seeded_repo().await;
        let agent_id = seed_test_agent(repo.as_ref()).await;
        let job_id = JobId::new("job-1");
        let originating_thread_id = ThreadId::new();
        seed_persisted_workflow_job(repo.as_ref(), &job_id, agent_id).await;

        let (release_tx, release_rx) = oneshot::channel();
        let provider: Arc<dyn LlmProvider> =
            Arc::new(ControlledProvider::new("Workflow complete", release_rx));
        let manager = JobManager::with_job_repository(
            Arc::new(TemplateManager::new(
                repo.clone() as Arc<dyn AgentRepository>,
                repo.clone(),
            )),
            Arc::new(StaticProviderResolver { provider }),
            Arc::new(ToolManager::new()),
            repo.clone() as Arc<dyn JobRepository>,
        );
        let (pipe_tx, mut pipe_rx) = broadcast::channel(8);
        let (control_tx, _control_rx) = mpsc::unbounded_channel();

        manager
            .spawn_persisted_job_executor(
                originating_thread_id,
                job_id.clone(),
                pipe_tx,
                control_tx,
            )
            .await
            .expect("spawn persisted job executor");

        wait_for_running_status(repo.as_ref(), &job_id).await;

        let running_job = JobRepository::get(repo.as_ref(), &job_id)
            .await
            .expect("load running job")
            .expect("job should exist");
        assert_eq!(running_job.status, WorkflowStatus::Running);
        assert!(running_job.result.is_none());

        release_tx.send(()).expect("release provider");

        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                match pipe_rx.recv().await.expect("receive job event") {
                    ThreadEvent::JobResult { job_id: event_job_id, .. }
                        if event_job_id == job_id.to_string() =>
                    {
                        break;
                    }
                    _ => {}
                }
            }
        })
        .await
        .expect("job result event should arrive");

        let completed_job = JobRepository::get(repo.as_ref(), &job_id)
            .await
            .expect("load completed job")
            .expect("job should exist");
        assert_eq!(completed_job.status, WorkflowStatus::Succeeded);
        let persisted_result = completed_job.result.expect("job result should be stored");
        assert!(persisted_result.success);
        assert_eq!(persisted_result.message, "Workflow complete");
        assert_eq!(persisted_result.agent_id, agent_id);
    }

    #[tokio::test]
    async fn spawn_job_executor_routes_persisted_workflow_jobs() {
        let repo = seeded_repo().await;
        let agent_id = seed_test_agent(repo.as_ref()).await;
        let job_id = JobId::new("job-routed");
        let originating_thread_id = ThreadId::new();
        seed_persisted_workflow_job(repo.as_ref(), &job_id, agent_id).await;

        let (release_tx, release_rx) = oneshot::channel();
        let provider: Arc<dyn LlmProvider> =
            Arc::new(ControlledProvider::new("Workflow complete", release_rx));
        let manager = JobManager::with_job_repository(
            Arc::new(TemplateManager::new(
                repo.clone() as Arc<dyn AgentRepository>,
                repo.clone(),
            )),
            Arc::new(StaticProviderResolver { provider }),
            Arc::new(ToolManager::new()),
            repo.clone() as Arc<dyn JobRepository>,
        );
        let (pipe_tx, mut pipe_rx) = broadcast::channel(8);
        let (control_tx, _control_rx) = mpsc::unbounded_channel();

        manager
            .spawn_job_executor(
                originating_thread_id,
                job_id.to_string(),
                AgentId::new(999),
                String::new(),
                None,
                pipe_tx,
                control_tx,
            )
            .await
            .expect("spawn_job_executor should route persisted workflow jobs");

        wait_for_running_status(repo.as_ref(), &job_id).await;
        release_tx.send(()).expect("release provider");

        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                match pipe_rx.recv().await.expect("receive job event") {
                    ThreadEvent::JobResult { job_id: event_job_id, .. }
                        if event_job_id == job_id.to_string() =>
                    {
                        break;
                    }
                    _ => {}
                }
            }
        })
        .await
        .expect("job result event should arrive");

        let completed_job = JobRepository::get(repo.as_ref(), &job_id)
            .await
            .expect("load completed job")
            .expect("job should exist");
        assert_eq!(completed_job.status, WorkflowStatus::Succeeded);
        let persisted_result = completed_job.result.expect("job result should be stored");
        assert!(persisted_result.success);
        assert_eq!(persisted_result.agent_id, agent_id);
    }

    #[tokio::test]
    async fn persisted_workflow_job_reports_failure_when_completion_persistence_fails() {
        let repo = seeded_repo().await;
        let agent_id = seed_test_agent(repo.as_ref()).await;
        let job_id = JobId::new("job-2");
        let originating_thread_id = ThreadId::new();
        seed_persisted_workflow_job(repo.as_ref(), &job_id, agent_id).await;

        let (release_tx, release_rx) = oneshot::channel();
        let provider: Arc<dyn LlmProvider> =
            Arc::new(ControlledProvider::new("Workflow complete", release_rx));
        let manager = JobManager::with_job_repository(
            Arc::new(TemplateManager::new(
                repo.clone() as Arc<dyn AgentRepository>,
                repo.clone(),
            )),
            Arc::new(StaticProviderResolver { provider }),
            Arc::new(ToolManager::new()),
            Arc::new(FailFirstResultWriteRepo::new(repo.clone())) as Arc<dyn JobRepository>,
        );
        let (pipe_tx, mut pipe_rx) = broadcast::channel(8);
        let (control_tx, _control_rx) = mpsc::unbounded_channel();

        manager
            .spawn_persisted_job_executor(
                originating_thread_id,
                job_id.clone(),
                pipe_tx,
                control_tx,
            )
            .await
            .expect("spawn persisted job executor");

        wait_for_running_status(repo.as_ref(), &job_id).await;
        release_tx.send(()).expect("release provider");

        let event = tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                match pipe_rx.recv().await.expect("receive job event") {
                    ref event @ ThreadEvent::JobResult {
                        job_id: ref event_job_id,
                        ..
                    } if event_job_id == job_id.as_ref() =>
                    {
                        break event.clone();
                    }
                    _ => {}
                }
            }
        })
        .await
        .expect("job result event should arrive");

        let ThreadEvent::JobResult {
            success,
            message,
            ..
        } = event
        else {
            unreachable!("filtered to job result");
        };
        assert!(!success);
        assert!(message.contains("failed to persist job completion"));

        let completed_job = JobRepository::get(repo.as_ref(), &job_id)
            .await
            .expect("load completed job")
            .expect("job should exist");
        assert_eq!(completed_job.status, WorkflowStatus::Failed);
        let persisted_result = completed_job.result.expect("job result should be stored");
        assert!(!persisted_result.success);
        assert!(persisted_result
            .message
            .contains("failed to persist job completion"));
    }
}
