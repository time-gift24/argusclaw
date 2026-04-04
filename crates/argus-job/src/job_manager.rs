//! JobManager for dispatching and managing background jobs.
//!
//! Each dispatched job is tracked through a ThreadPool-managed execution thread.
//! Results are sent back through the unified pipe as ThreadEvent::JobResult.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex as StdMutex};

use argus_agent::TurnCancellation;
#[cfg(test)]
use argus_agent::TurnRecord;
#[cfg(test)]
use argus_protocol::llm::{ChatMessage, Role};
use argus_protocol::{
    AgentId, MailboxMessage, MailboxMessageType, ProviderResolver, ThreadEvent, ThreadId,
    ThreadJobResult, ThreadPoolRuntimeKind, ThreadPoolRuntimeRef, ThreadPoolSnapshot,
    ThreadPoolState, ThreadRuntimeStatus,
};
use argus_repository::traits::{JobRepository, LlmProviderRepository, ThreadRepository};
use argus_repository::types::{JobId, JobStatus};
use argus_template::TemplateManager;
use argus_tool::ToolManager;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::error::JobError;
use crate::thread_pool::{ThreadPool, ThreadPoolPersistence};
use crate::types::ThreadPoolJobRequest;

#[derive(Debug, Clone)]
enum TrackedJobState {
    Pending,
    Cancelling,
    Completed(ThreadJobResult),
    Consumed(ThreadJobResult),
}

#[derive(Debug, Clone)]
struct TrackedJob {
    thread_id: ThreadId,
    state: TrackedJobState,
    /// Cancellation handle for Pending jobs; None once Completed/Consumed.
    cancellation: Option<TurnCancellation>,
    generation: u64,
}

#[derive(Debug, Default)]
struct TrackedJobsStore {
    jobs: HashMap<String, TrackedJob>,
    terminal_order: VecDeque<(String, u64)>,
    next_generation: u64,
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
    thread_pool: Arc<ThreadPool>,
    tracked_jobs: Arc<StdMutex<TrackedJobsStore>>,
    chat_mailbox_forwarder: Arc<StdMutex<Option<Arc<ChatMailboxForwarder>>>>,
    job_repository: Option<Arc<dyn JobRepository>>,
}

type ChatMailboxForwarderFuture = Pin<Box<dyn Future<Output = bool> + Send>>;
type ChatMailboxForwarder =
    dyn Fn(ThreadId, MailboxMessage) -> ChatMailboxForwarderFuture + Send + Sync;

impl fmt::Debug for JobManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobManager").finish()
    }
}

impl JobManager {
    const TERMINAL_JOB_RETENTION_LIMIT: usize = 1024;
    #[cfg(test)]
    const JOB_RESULT_SUMMARY_CHAR_LIMIT: usize = 4000;

    /// Create a new JobManager.
    pub fn new(
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        tool_manager: Arc<ToolManager>,
        trace_dir: PathBuf,
    ) -> Self {
        Self::new_with_persistence(
            template_manager,
            provider_resolver,
            tool_manager,
            trace_dir,
            None,
        )
    }

    /// Create a new JobManager with optional persistent thread-pool backing.
    pub fn new_with_persistence(
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        tool_manager: Arc<ToolManager>,
        trace_dir: PathBuf,
        persistence: Option<ThreadPoolPersistence>,
    ) -> Self {
        let job_repository = persistence
            .as_ref()
            .map(ThreadPoolPersistence::job_repository);
        let thread_pool = Arc::new(ThreadPool::with_persistence(
            template_manager,
            provider_resolver,
            tool_manager,
            trace_dir,
            persistence,
        ));

        Self {
            thread_pool,
            tracked_jobs: Arc::new(StdMutex::new(TrackedJobsStore::default())),
            chat_mailbox_forwarder: Arc::new(StdMutex::new(None)),
            job_repository,
        }
    }

    /// Create a new JobManager wired with repository-backed persistence.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_repositories(
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        tool_manager: Arc<ToolManager>,
        trace_dir: PathBuf,
        job_repository: Arc<dyn JobRepository>,
        thread_repository: Arc<dyn ThreadRepository>,
        provider_repository: Arc<dyn LlmProviderRepository>,
    ) -> Self {
        Self::new_with_persistence(
            template_manager,
            provider_resolver,
            tool_manager,
            trace_dir,
            Some(ThreadPoolPersistence::new(
                job_repository,
                thread_repository,
                provider_repository,
            )),
        )
    }

    /// Get the currently bound execution thread for a job, if any.
    pub fn thread_binding(&self, job_id: &str) -> Option<ThreadId> {
        self.thread_pool.get_thread_binding(job_id)
    }

    /// Return the shared unified thread pool.
    pub fn thread_pool(&self) -> Arc<ThreadPool> {
        Arc::clone(&self.thread_pool)
    }

    pub fn set_chat_mailbox_forwarder<F, Fut>(&self, forwarder: F)
    where
        F: Fn(ThreadId, MailboxMessage) -> Fut + Send + Sync + 'static,
        Fut: Future<Output = bool> + Send + 'static,
    {
        let forwarder = Arc::new(
            move |thread_id: ThreadId, message: MailboxMessage| -> ChatMailboxForwarderFuture {
                Box::pin(forwarder(thread_id, message))
            },
        ) as Arc<ChatMailboxForwarder>;
        let mut slot = self
            .chat_mailbox_forwarder
            .lock()
            .expect("chat mailbox forwarder mutex poisoned");
        *slot = Some(forwarder);
    }

    /// Collect a point-in-time thread-pool snapshot.
    pub fn thread_pool_snapshot(&self) -> ThreadPoolSnapshot {
        self.thread_pool.collect_metrics()
    }

    /// Collect the authoritative thread-pool state.
    pub fn thread_pool_state(&self) -> ThreadPoolState {
        self.thread_pool.collect_state()
    }

    /// Stop a running background job by signalling cancellation.
    ///
    /// Returns `JobNotFound` if the job was never dispatched,
    /// or `JobNotRunning` if it already completed.
    pub fn stop_job(&self, job_id: &str) -> Result<(), JobError> {
        let cancellation = {
            let mut tracked_jobs = self
                .tracked_jobs
                .lock()
                .expect("job tracking mutex poisoned");

            let tracked_job = tracked_jobs
                .jobs
                .get_mut(job_id)
                .ok_or_else(|| JobError::JobNotFound(job_id.to_string()))?;

            match &tracked_job.state {
                TrackedJobState::Pending => {}
                TrackedJobState::Cancelling
                | TrackedJobState::Completed(_)
                | TrackedJobState::Consumed(_) => {
                    return Err(JobError::JobNotRunning(job_id.to_string()));
                }
            }

            if tracked_job.cancellation.is_none() || !self.is_job_runtime_active(job_id) {
                return Err(JobError::JobNotRunning(job_id.to_string()));
            }

            let cancellation = tracked_job
                .cancellation
                .take()
                .ok_or_else(|| JobError::JobNotRunning(job_id.to_string()))?;
            tracked_job.state = TrackedJobState::Cancelling;
            cancellation
        };

        cancellation.cancel();

        Ok(())
    }

    /// Record that a job was dispatched for a thread.
    pub fn record_dispatched_job(&self, thread_id: ThreadId, job_id: String) {
        Self::record_dispatched_job_in_store(
            &self.tracked_jobs,
            thread_id,
            job_id,
            TurnCancellation::new(),
        );
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
        Self::lookup_job_in_store(&mut tracked_jobs, thread_id, job_id, consume)
    }

    /// Get the current status for a job, recovering persisted state when caches are cold.
    pub async fn get_job_result_status_persisted(
        &self,
        thread_id: ThreadId,
        job_id: &str,
        consume: bool,
    ) -> Result<JobLookup, JobError> {
        {
            let mut tracked_jobs = self
                .tracked_jobs
                .lock()
                .expect("job tracking mutex poisoned");
            let lookup = Self::lookup_job_in_store(&mut tracked_jobs, thread_id, job_id, consume);
            if !matches!(lookup, JobLookup::NotFound) {
                return Ok(lookup);
            }
        }

        let Some(job_repository) = &self.job_repository else {
            return Ok(JobLookup::NotFound);
        };
        let Some(job_record) = job_repository
            .get(&JobId::new(job_id))
            .await
            .map_err(|err| {
                JobError::ExecutionFailed(format!("failed to load job record: {err}"))
            })?
        else {
            return Ok(JobLookup::NotFound);
        };
        let Some(execution_thread_id) = job_record.thread_id else {
            return Ok(JobLookup::NotFound);
        };
        let Some(metadata) = self
            .thread_pool
            .recover_job_thread_metadata(execution_thread_id)
            .await?
        else {
            return Ok(JobLookup::NotFound);
        };
        if metadata.parent_thread_id != Some(thread_id)
            || metadata.job_id.as_deref() != Some(job_id)
        {
            return Ok(JobLookup::NotFound);
        }

        match job_record.status {
            JobStatus::Pending | JobStatus::Queued | JobStatus::Running => Ok(JobLookup::Pending),
            JobStatus::Succeeded | JobStatus::Failed | JobStatus::Cancelled => {
                let Some(result) = job_record.result else {
                    return Ok(JobLookup::NotFound);
                };
                let persisted = ThreadJobResult {
                    job_id: job_id.to_string(),
                    success: result.success,
                    message: result.message,
                    token_usage: result.token_usage,
                    agent_id: AgentId::new(result.agent_id.inner()),
                    agent_display_name: result.agent_display_name,
                    agent_description: result.agent_description,
                };
                Self::record_completed_job_result_in_store(
                    &self.tracked_jobs,
                    thread_id,
                    persisted.clone(),
                );
                let mut tracked_jobs = self
                    .tracked_jobs
                    .lock()
                    .expect("job tracking mutex poisoned");
                Ok(Self::lookup_job_in_store(
                    &mut tracked_jobs,
                    thread_id,
                    job_id,
                    consume,
                ))
            }
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
    ) -> Result<(), JobError> {
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

        let cancellation = TurnCancellation::new();
        let spawn_cancellation = cancellation.clone();
        Self::record_dispatched_job_in_store(
            &self.tracked_jobs,
            originating_thread_id,
            job_id.clone(),
            cancellation,
        );
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
        let chat_mailbox_forwarder = Arc::clone(&self.chat_mailbox_forwarder);
        let pipe_tx_clone = pipe_tx.clone();

        tokio::spawn(async move {
            let result = thread_pool
                .execute_job(
                    request,
                    execution_thread_id,
                    pipe_tx_clone.clone(),
                    spawn_cancellation,
                )
                .await;

            Self::forward_job_result_to_runtime(
                &thread_pool,
                &chat_mailbox_forwarder,
                originating_thread_id,
                execution_thread_id,
                result.clone(),
            )
            .await;
            Self::record_completed_job_result_in_store(
                &tracked_jobs,
                originating_thread_id,
                result.clone(),
            );
            Self::broadcast_job_result(&pipe_tx_clone, originating_thread_id, result);
        });

        Ok(())
    }

    /// Summarize turn output into a brief result message.
    #[cfg(test)]
    fn summarize_output(output: &TurnRecord) -> String {
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

    fn record_dispatched_job_in_store(
        tracked_jobs: &Arc<StdMutex<TrackedJobsStore>>,
        thread_id: ThreadId,
        job_id: String,
        cancellation: TurnCancellation,
    ) {
        let mut tracked_jobs = tracked_jobs.lock().expect("job tracking mutex poisoned");
        let generation = tracked_jobs.next_generation;
        tracked_jobs.next_generation = tracked_jobs.next_generation.saturating_add(1);
        tracked_jobs.jobs.insert(
            job_id,
            TrackedJob {
                thread_id,
                state: TrackedJobState::Pending,
                cancellation: Some(cancellation),
                generation,
            },
        );
    }

    fn record_completed_job_result_in_store(
        tracked_jobs: &Arc<StdMutex<TrackedJobsStore>>,
        thread_id: ThreadId,
        result: ThreadJobResult,
    ) {
        let mut tracked_jobs = tracked_jobs.lock().expect("job tracking mutex poisoned");
        let generation = tracked_jobs.next_generation;
        tracked_jobs.next_generation = tracked_jobs.next_generation.saturating_add(1);
        let job_id = result.job_id.clone();
        tracked_jobs.jobs.insert(
            job_id.clone(),
            TrackedJob {
                thread_id,
                state: TrackedJobState::Completed(result),
                cancellation: None,
                generation,
            },
        );
        tracked_jobs.terminal_order.push_back((job_id, generation));
        Self::prune_terminal_jobs(&mut tracked_jobs);
    }

    fn prune_terminal_jobs(tracked_jobs: &mut TrackedJobsStore) {
        while tracked_jobs.terminal_order.len() > Self::TERMINAL_JOB_RETENTION_LIMIT {
            let Some((job_id, generation)) = tracked_jobs.terminal_order.pop_front() else {
                break;
            };
            let should_remove = tracked_jobs.jobs.get(&job_id).is_some_and(|tracked_job| {
                tracked_job.generation == generation
                    && matches!(
                        tracked_job.state,
                        TrackedJobState::Completed(_) | TrackedJobState::Consumed(_)
                    )
            });
            if should_remove {
                tracked_jobs.jobs.remove(&job_id);
            }
        }
    }

    fn lookup_job_in_store(
        tracked_jobs: &mut TrackedJobsStore,
        thread_id: ThreadId,
        job_id: &str,
        consume: bool,
    ) -> JobLookup {
        let Some(tracked_job) = tracked_jobs.jobs.get_mut(job_id) else {
            return JobLookup::NotFound;
        };

        if tracked_job.thread_id != thread_id {
            return JobLookup::NotFound;
        }

        match &tracked_job.state {
            TrackedJobState::Pending | TrackedJobState::Cancelling => JobLookup::Pending,
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

    fn is_job_runtime_active(&self, job_id: &str) -> bool {
        let Some(thread_id) = self.thread_pool.get_thread_binding(job_id) else {
            return true;
        };

        self.thread_pool
            .collect_state()
            .runtimes
            .into_iter()
            .find(|runtime| runtime.runtime.thread_id == thread_id)
            .is_some_and(|runtime| {
                matches!(
                    runtime.status,
                    ThreadRuntimeStatus::Queued | ThreadRuntimeStatus::Running
                )
            })
    }

    async fn forward_job_result_to_runtime(
        thread_pool: &ThreadPool,
        chat_mailbox_forwarder: &Arc<StdMutex<Option<Arc<ChatMailboxForwarder>>>>,
        originating_thread_id: ThreadId,
        execution_thread_id: ThreadId,
        result: ThreadJobResult,
    ) {
        let mailbox_message = MailboxMessage {
            id: Uuid::new_v4().to_string(),
            from_thread_id: execution_thread_id,
            to_thread_id: originating_thread_id,
            from_label: result.agent_display_name.clone(),
            message_type: MailboxMessageType::JobResult {
                job_id: result.job_id.clone(),
                success: result.success,
                token_usage: result.token_usage.clone(),
                agent_id: result.agent_id,
                agent_display_name: result.agent_display_name.clone(),
                agent_description: result.agent_description.clone(),
            },
            text: result.message.clone(),
            timestamp: chrono::Utc::now().to_rfc3339(),
            read: false,
            summary: None,
        };
        let forwarder = chat_mailbox_forwarder
            .lock()
            .expect("chat mailbox forwarder mutex poisoned")
            .clone();
        let forwarded = match forwarder {
            Some(forwarder) => forwarder(originating_thread_id, mailbox_message.clone()).await,
            None => false,
        };
        if !forwarded {
            let _ = thread_pool
                .deliver_mailbox_message(originating_thread_id, mailbox_message)
                .await;
        }
    }

    pub fn is_job_pending(&self, job_id: &str) -> bool {
        let tracked_jobs = self
            .tracked_jobs
            .lock()
            .expect("job tracking mutex poisoned");

        tracked_jobs
            .jobs
            .get(job_id)
            .is_some_and(|tracked_job| matches!(tracked_job.state, TrackedJobState::Pending))
    }

    pub async fn is_job_pending_persisted(&self, job_id: &str) -> Result<bool, JobError> {
        {
            let tracked_jobs = self
                .tracked_jobs
                .lock()
                .expect("job tracking mutex poisoned");
            if let Some(tracked_job) = tracked_jobs.jobs.get(job_id) {
                return Ok(matches!(
                    tracked_job.state,
                    TrackedJobState::Pending | TrackedJobState::Cancelling
                ));
            }
        }

        let Some(job_repository) = &self.job_repository else {
            return Ok(false);
        };
        let Some(job_record) = job_repository
            .get(&JobId::new(job_id))
            .await
            .map_err(|err| {
                JobError::ExecutionFailed(format!("failed to load job record: {err}"))
            })?
        else {
            return Ok(false);
        };

        Ok(matches!(
            job_record.status,
            JobStatus::Pending | JobStatus::Queued | JobStatus::Running
        ))
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
    use std::sync::Arc;

    use argus_agent::thread_trace_store::{
        ThreadTraceKind, ThreadTraceMetadata, chat_thread_base_dir, persist_thread_metadata,
    };
    use argus_protocol::llm::{
        CompletionRequest, CompletionResponse, FinishReason, LlmError, LlmProviderRepository,
    };
    use argus_protocol::{
        AgentRecord, AgentType, LlmProvider, ProviderId, SessionId, ThinkingConfig, ThreadId,
        ThreadRuntimeStatus,
    };
    use argus_repository::ArgusSqlite;
    use argus_repository::migrate;
    use argus_repository::traits::{
        AgentRepository, JobRepository, SessionRepository, ThreadRepository,
    };
    use argus_repository::types::{AgentId as RepoAgentId, ThreadRecord};
    use argus_template::TemplateManager;
    use async_trait::async_trait;
    use rust_decimal::Decimal;
    use sqlx::SqlitePool;

    use argus_protocol::TokenUsage;
    use argus_tool::ToolManager;
    use tokio::time::{Duration, sleep, timeout};

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
            std::env::temp_dir().join("argus-job-tests"),
        )
    }

    struct FixedProviderResolver {
        provider: Arc<dyn LlmProvider>,
    }

    impl FixedProviderResolver {
        fn new(provider: Arc<dyn LlmProvider>) -> Self {
            Self { provider }
        }
    }

    #[async_trait]
    impl ProviderResolver for FixedProviderResolver {
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

    #[derive(Debug)]
    struct CapturingProvider {
        response: String,
        delay: Duration,
        token_count: u32,
    }

    impl CapturingProvider {
        fn new(response: &str, delay: Duration, token_count: u32) -> Self {
            Self {
                response: response.to_string(),
                delay,
                token_count,
            }
        }
    }

    #[async_trait]
    impl LlmProvider for CapturingProvider {
        fn model_name(&self) -> &str {
            "capturing"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> std::result::Result<CompletionResponse, LlmError> {
            sleep(self.delay).await;
            Ok(CompletionResponse {
                content: Some(self.response.clone()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: self.token_count,
                output_tokens: self.token_count / 2,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            })
        }
    }

    async fn test_job_manager_with_provider(
        provider: Arc<dyn LlmProvider>,
    ) -> (JobManager, AgentId, ThreadId) {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("sqlite memory pool should connect");
        migrate(&pool).await.expect("migration should succeed");
        let sqlite = Arc::new(ArgusSqlite::new(pool));
        let template_manager = Arc::new(TemplateManager::new(
            sqlite.clone() as Arc<dyn AgentRepository>,
            sqlite.clone(),
        ));
        let agent_id = AgentId::new(7);
        let agent_record = AgentRecord {
            id: agent_id,
            display_name: "Cancellable Job Agent".to_string(),
            description: "Used to test stop_job cancellation".to_string(),
            version: "1.0.0".to_string(),
            provider_id: Some(ProviderId::new(1)),
            model_id: Some("capturing".to_string()),
            system_prompt: "You are a cancellable test agent.".to_string(),
            tool_names: vec![],
            max_tokens: None,
            temperature: None,
            thinking_config: Some(ThinkingConfig::enabled()),
            parent_agent_id: None,
            agent_type: AgentType::Standard,
        };
        template_manager
            .upsert(agent_record.clone())
            .await
            .expect("agent upsert should succeed");

        let trace_dir =
            std::env::temp_dir().join(format!("argus-job-tests-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&trace_dir).expect("trace dir should exist");
        let parent_session_id = SessionId::new();
        let parent_thread_id = ThreadId::new();
        SessionRepository::create(sqlite.as_ref(), &parent_session_id, "job-parent")
            .await
            .expect("parent session should persist");
        ThreadRepository::upsert_thread(
            sqlite.as_ref(),
            &ThreadRecord {
                id: parent_thread_id,
                provider_id: argus_protocol::LlmProviderId::new(1),
                title: Some("job-parent".to_string()),
                token_count: 0,
                turn_count: 0,
                session_id: Some(parent_session_id),
                template_id: Some(RepoAgentId::new(agent_id.inner())),
                model_override: Some("capturing".to_string()),
                created_at: chrono::Utc::now().to_rfc3339(),
                updated_at: chrono::Utc::now().to_rfc3339(),
            },
        )
        .await
        .expect("parent thread should persist");
        persist_thread_metadata(
            &chat_thread_base_dir(&trace_dir, parent_session_id, parent_thread_id),
            &ThreadTraceMetadata {
                thread_id: parent_thread_id,
                kind: ThreadTraceKind::ChatRoot,
                root_session_id: Some(parent_session_id),
                parent_thread_id: None,
                job_id: None,
                agent_snapshot: agent_record,
            },
        )
        .await
        .expect("parent trace metadata should persist");

        (
            JobManager::new_with_repositories(
                template_manager,
                Arc::new(FixedProviderResolver::new(provider)),
                Arc::new(ToolManager::new()),
                trace_dir,
                sqlite.clone() as Arc<dyn JobRepository>,
                sqlite.clone() as Arc<dyn ThreadRepository>,
                sqlite as Arc<dyn LlmProviderRepository>,
            ),
            agent_id,
            parent_thread_id,
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
            std::env::temp_dir().join("argus-job-tests"),
            sqlite.clone() as Arc<dyn JobRepository>,
            sqlite.clone() as Arc<dyn ThreadRepository>,
            sqlite as Arc<dyn LlmProviderRepository>,
        )
    }

    fn assistant_output(content: &str) -> TurnRecord {
        TurnRecord::user_turn(
            1,
            vec![ChatMessage::assistant(content)],
            TokenUsage::default(),
        )
    }

    fn sample_job_result(job_id: impl Into<String>) -> ThreadJobResult {
        ThreadJobResult {
            job_id: job_id.into(),
            success: true,
            message: "all done".to_string(),
            token_usage: None,
            agent_id: AgentId::new(9),
            agent_display_name: "Researcher".to_string(),
            agent_description: "Looks things up".to_string(),
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
        let result = sample_job_result("job-42");

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
    async fn tracked_job_store_prunes_oldest_terminal_results() {
        let manager = test_job_manager();
        let thread_id = ThreadId::new();

        for index in 0..1030 {
            let result = sample_job_result(format!("job-terminal-{index}"));
            manager.record_completed_job_result(thread_id, result);
        }

        assert!(matches!(
            manager.get_job_result_status(thread_id, "job-terminal-0", false),
            JobLookup::NotFound
        ));
        assert!(matches!(
            manager.get_job_result_status(thread_id, "job-terminal-1029", false),
            JobLookup::Completed(found) if found.job_id == "job-terminal-1029"
        ));
    }

    #[tokio::test]
    async fn tracked_job_store_prunes_consumed_results_after_retention_window() {
        let manager = test_job_manager();
        let thread_id = ThreadId::new();
        let oldest = sample_job_result("job-consumed-oldest");

        manager.record_completed_job_result(thread_id, oldest.clone());
        assert!(matches!(
            manager.get_job_result_status(thread_id, &oldest.job_id, true),
            JobLookup::Completed(found) if found.job_id == oldest.job_id
        ));
        assert!(matches!(
            manager.get_job_result_status(thread_id, &oldest.job_id, false),
            JobLookup::Consumed(found) if found.job_id == oldest.job_id
        ));

        for index in 0..1030 {
            let result = sample_job_result(format!("job-consumed-fill-{index}"));
            manager.record_completed_job_result(thread_id, result);
        }

        assert!(matches!(
            manager.get_job_result_status(thread_id, &oldest.job_id, false),
            JobLookup::NotFound
        ));
    }

    #[tokio::test]
    async fn tracked_job_store_never_prunes_pending_or_cancelling_entries() {
        let manager = test_job_manager();
        let thread_id = ThreadId::new();
        let pending_job_id = "job-pending-retained".to_string();
        let cancelling_job_id = "job-cancelling-retained".to_string();

        manager.record_dispatched_job(thread_id, pending_job_id.clone());
        manager.record_dispatched_job(thread_id, cancelling_job_id.clone());
        manager
            .stop_job(&cancelling_job_id)
            .expect("stop_job should move tracked state to cancelling");

        for index in 0..1030 {
            let result = sample_job_result(format!("job-retention-fill-{index}"));
            manager.record_completed_job_result(thread_id, result);
        }

        assert!(matches!(
            manager.get_job_result_status(thread_id, &pending_job_id, false),
            JobLookup::Pending
        ));
        assert!(matches!(
            manager.get_job_result_status(thread_id, &cancelling_job_id, false),
            JobLookup::Pending
        ));
    }

    #[tokio::test]
    async fn dispatch_job_creates_thread_pool_binding() {
        let manager = test_job_manager();
        let originating_thread_id = ThreadId::new();
        let (pipe_tx, _pipe_rx) = broadcast::channel(16);
        let job_id = "job-bound".to_string();

        manager
            .dispatch_job(
                originating_thread_id,
                job_id.clone(),
                AgentId::new(99),
                "run this".to_string(),
                None,
                pipe_tx,
            )
            .await
            .expect("job should enqueue even if execution later fails");

        let bound_thread_id = manager
            .thread_binding(&job_id)
            .expect("job should be bound to a thread");
        let runtime = manager
            .thread_pool_state()
            .runtimes
            .into_iter()
            .find(|runtime| runtime.runtime.thread_id == bound_thread_id)
            .expect("bound runtime should be tracked in thread pool state");
        assert_eq!(runtime.runtime.job_id.as_deref(), Some(job_id.as_str()));
        assert!(matches!(
            runtime.status,
            argus_protocol::ThreadRuntimeStatus::Queued
                | argus_protocol::ThreadRuntimeStatus::Running
                | argus_protocol::ThreadRuntimeStatus::Cooling
        ));
    }

    #[tokio::test]
    async fn alpha_dispatch_job_emits_binding_queue_metrics_and_result_events() {
        let manager = test_job_manager();
        let originating_thread_id = ThreadId::new();
        let (pipe_tx, mut pipe_rx) = broadcast::channel(32);
        let job_id = "alpha-job-event-flow".to_string();

        manager
            .dispatch_job(
                originating_thread_id,
                job_id.clone(),
                AgentId::new(99),
                "run alpha event flow".to_string(),
                None,
                pipe_tx,
            )
            .await
            .expect("job should enqueue even if execution later fails");

        let mut bound_thread_id = None;
        let mut saw_queued = false;
        let mut saw_metrics = false;
        let mut saw_result = false;

        timeout(Duration::from_secs(5), async {
            while !saw_result {
                match pipe_rx.recv().await {
                    Ok(ThreadEvent::ThreadBoundToJob {
                        job_id: event_job_id,
                        thread_id: execution_thread_id,
                    }) if event_job_id == job_id => {
                        assert_ne!(execution_thread_id, originating_thread_id);
                        bound_thread_id = Some(execution_thread_id);
                    }
                    Ok(ThreadEvent::ThreadPoolQueued { runtime })
                        if runtime.job_id.as_deref() == Some(job_id.as_str()) =>
                    {
                        assert_eq!(runtime.kind, ThreadPoolRuntimeKind::Job);
                        if let Some(execution_thread_id) = bound_thread_id {
                            assert_eq!(runtime.thread_id, execution_thread_id);
                        }
                        saw_queued = true;
                    }
                    Ok(ThreadEvent::ThreadPoolMetricsUpdated { .. }) => {
                        saw_metrics = true;
                    }
                    Ok(ThreadEvent::JobResult {
                        thread_id,
                        job_id: event_job_id,
                        success,
                        ..
                    }) if event_job_id == job_id => {
                        assert_eq!(thread_id, originating_thread_id);
                        assert!(
                            !success,
                            "alpha flow should surface execution failure when the agent record is missing"
                        );
                        saw_result = true;
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        panic!("thread event channel should remain open");
                    }
                }
            }
        })
        .await
        .expect("job result event should arrive");

        let execution_thread_id = bound_thread_id.expect("job should bind to an execution thread");
        assert_eq!(manager.thread_binding(&job_id), Some(execution_thread_id));
        assert!(saw_queued, "queued event should be observed");
        assert!(saw_metrics, "metrics update should be observed");
    }

    #[tokio::test]
    async fn dispatch_job_enqueue_failure_does_not_leave_pending_tracking() {
        let manager = test_persistent_job_manager_without_default_provider().await;
        let originating_thread_id = ThreadId::new();
        let (pipe_tx, _pipe_rx) = broadcast::channel(16);
        let job_id = "job-enqueue-failure".to_string();

        let dispatch_result = manager
            .dispatch_job(
                originating_thread_id,
                job_id.clone(),
                AgentId::new(999_999),
                "run this".to_string(),
                None,
                pipe_tx,
            )
            .await;

        assert!(dispatch_result.is_err());
        assert!(matches!(
            manager.get_job_result_status(originating_thread_id, &job_id, false),
            JobLookup::NotFound
        ));
    }

    #[tokio::test]
    async fn stop_job_signals_cancellation_for_pending_job() {
        let manager = test_job_manager();
        let thread_id = ThreadId::new();
        let job_id = "job-stop-pending".to_string();

        manager.record_dispatched_job(thread_id, job_id.clone());

        assert!(matches!(
            manager.get_job_result_status(thread_id, &job_id, false),
            JobLookup::Pending
        ));

        manager
            .stop_job(&job_id)
            .expect("stop_job should succeed for pending job");
    }

    #[tokio::test]
    async fn stop_job_returns_not_running_after_stop_already_requested() {
        let manager = test_job_manager();
        let thread_id = ThreadId::new();
        let job_id = "job-stop-repeat".to_string();

        manager.record_dispatched_job(thread_id, job_id.clone());
        manager
            .stop_job(&job_id)
            .expect("first stop_job should succeed");

        let error = manager
            .stop_job(&job_id)
            .expect_err("second stop_job should report not running");
        assert!(matches!(error, JobError::JobNotRunning(found) if found == job_id));
    }

    #[tokio::test]
    async fn stop_job_returns_not_found_for_unknown_job() {
        let manager = test_job_manager();

        let result = manager.stop_job("nonexistent-job");
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("job not found"));
    }

    #[tokio::test]
    async fn stop_job_cancels_turn_and_broadcasts_failed_job_result() {
        let provider = Arc::new(CapturingProvider::new(
            "delayed reply",
            Duration::from_secs(5),
            24,
        ));
        let (manager, agent_id, originating_thread_id) =
            test_job_manager_with_provider(provider).await;
        let job_id = "job-stop-end-to-end".to_string();
        let (pipe_tx, mut pipe_rx) = broadcast::channel(32);

        manager
            .dispatch_job(
                originating_thread_id,
                job_id.clone(),
                agent_id,
                "please take your time".to_string(),
                None,
                pipe_tx,
            )
            .await
            .expect("dispatch should succeed");

        timeout(Duration::from_secs(5), async {
            loop {
                let status = manager
                    .thread_pool_state()
                    .runtimes
                    .into_iter()
                    .find(|runtime| runtime.runtime.job_id.as_deref() == Some(job_id.as_str()))
                    .map(|runtime| runtime.status);
                if matches!(
                    status,
                    Some(ThreadRuntimeStatus::Queued | ThreadRuntimeStatus::Running)
                ) {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("job runtime should become active");

        manager
            .stop_job(&job_id)
            .expect("stop_job should succeed while job is active");

        let job_event = timeout(Duration::from_secs(5), async {
            loop {
                match pipe_rx.recv().await {
                    Ok(ThreadEvent::JobResult {
                        thread_id,
                        job_id: event_job_id,
                        success,
                        message,
                        ..
                    }) if event_job_id == job_id => {
                        assert_eq!(thread_id, originating_thread_id);
                        break (success, message);
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        panic!("thread event channel should remain open");
                    }
                }
            }
        })
        .await
        .expect("job result event should arrive");

        assert!(!job_event.0, "cancelled job should report failure");
        assert!(
            job_event.1.contains("Turn cancelled"),
            "unexpected cancel message: {}",
            job_event.1
        );

        assert!(matches!(
            manager.get_job_result_status(originating_thread_id, &job_id, false),
            JobLookup::Completed(ThreadJobResult { success: false, .. })
        ));
    }
}
