//! JobManager for dispatching and managing background jobs.
//!
//! Each dispatched job is tracked through a ThreadPool-managed execution thread.
//! Results are sent back through the unified pipe as ThreadEvent::JobResult.

use std::collections::{HashMap, VecDeque};
use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex as StdMutex, Weak};

#[cfg(test)]
use argus_agent::TurnRecord;
use argus_agent::thread_bootstrap::{
    build_thread_config, cleanup_trace_dir, hydrate_turn_log_state, recover_and_validate_metadata,
};
use argus_agent::thread_trace_store::{
    ThreadTraceKind, ThreadTraceMetadata, child_thread_base_dir, find_job_thread_base_dir,
    list_direct_child_threads, persist_thread_metadata, recover_thread_metadata,
};
use argus_agent::{
    FilePlanStore, LlmThreadCompactor, Thread, ThreadBuilder, ThreadHandle, TurnCancellation,
};
use argus_protocol::llm::{ChatMessage, LlmProvider, Role};
use argus_protocol::{
    AgentId, JobRuntimeSnapshot, JobRuntimeState, JobRuntimeSummary, MailboxMessage,
    MailboxMessageType, McpToolResolver, ProviderId, ProviderResolver, SessionId, ThreadEvent,
    ThreadId, ThreadJobResult, ThreadMessage, ThreadPoolEventReason, ThreadRuntimeStatus,
};
use argus_repository::traits::{JobRepository, LlmProviderRepository, ThreadRepository};
use argus_repository::types::{
    AgentId as RepoAgentId, JobId, JobRecord, JobResult, JobStatus, JobType, ThreadRecord,
};
use argus_template::TemplateManager;
use argus_thread_pool::{RuntimeLifecycleChange, ThreadPool, ThreadPoolError};
use argus_tool::ToolManager;
use chrono::Utc;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::error::JobError;
use crate::types::{JobExecutionRequest, RecoveredChildJob};

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

#[derive(Debug, Default)]
struct JobRuntimeStore {
    job_bindings: HashMap<String, ThreadId>,
    parent_thread_by_child: HashMap<ThreadId, ThreadId>,
    child_jobs_by_parent: HashMap<ThreadId, Vec<RecoveredChildJob>>,
    delivered_job_results: HashMap<ThreadId, Vec<MailboxMessage>>,
    job_runtimes: HashMap<ThreadId, JobRuntimeSummary>,
    peak_estimated_memory_bytes: u64,
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
#[derive(Clone)]
pub struct JobManager {
    thread_pool: Arc<ThreadPool>,
    template_manager: Arc<TemplateManager>,
    provider_resolver: Arc<dyn ProviderResolver>,
    tool_manager: Arc<ToolManager>,
    trace_dir: PathBuf,
    mcp_tool_resolver: Arc<StdMutex<Option<Arc<dyn McpToolResolver>>>>,
    thread_repository: Option<Arc<dyn ThreadRepository>>,
    provider_repository: Option<Arc<dyn LlmProviderRepository>>,
    tracked_jobs: Arc<StdMutex<TrackedJobsStore>>,
    job_runtime_store: Arc<StdMutex<JobRuntimeStore>>,
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
        thread_pool: Arc<ThreadPool>,
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        tool_manager: Arc<ToolManager>,
        trace_dir: PathBuf,
    ) -> Self {
        Self::new_with_repositories(
            thread_pool,
            template_manager,
            provider_resolver,
            tool_manager,
            trace_dir,
            None,
            None,
            None,
        )
    }

    /// Create a new JobManager with optional repository backing.
    pub fn new_with_persistence(
        thread_pool: Arc<ThreadPool>,
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        tool_manager: Arc<ToolManager>,
        trace_dir: PathBuf,
        job_repository: Option<Arc<dyn JobRepository>>,
        thread_repository: Option<Arc<dyn ThreadRepository>>,
        provider_repository: Option<Arc<dyn LlmProviderRepository>>,
    ) -> Self {
        let manager = Self {
            thread_pool,
            template_manager,
            provider_resolver,
            tool_manager,
            trace_dir,
            mcp_tool_resolver: Arc::new(StdMutex::new(None)),
            thread_repository,
            provider_repository,
            tracked_jobs: Arc::new(StdMutex::new(TrackedJobsStore::default())),
            job_runtime_store: Arc::new(StdMutex::new(JobRuntimeStore::default())),
            chat_mailbox_forwarder: Arc::new(StdMutex::new(None)),
            job_repository,
        };
        manager.install_runtime_lifecycle_bridge();
        manager
    }

    /// Create a new JobManager wired with repository-backed persistence.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_repositories(
        thread_pool: Arc<ThreadPool>,
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        tool_manager: Arc<ToolManager>,
        trace_dir: PathBuf,
        job_repository: Option<Arc<dyn JobRepository>>,
        thread_repository: Option<Arc<dyn ThreadRepository>>,
        provider_repository: Option<Arc<dyn LlmProviderRepository>>,
    ) -> Self {
        Self::new_with_persistence(
            thread_pool,
            template_manager,
            provider_resolver,
            tool_manager,
            trace_dir,
            job_repository,
            thread_repository,
            provider_repository,
        )
    }

    /// Get the currently bound execution thread for a job, if any.
    pub fn thread_binding(&self, job_id: &str) -> Option<ThreadId> {
        self.job_runtime_store
            .lock()
            .expect("job runtime mutex poisoned")
            .job_bindings
            .get(job_id)
            .copied()
    }

    pub fn parent_job_thread_id(&self, child_thread_id: &ThreadId) -> Option<ThreadId> {
        self.job_runtime_store
            .lock()
            .expect("job runtime mutex poisoned")
            .parent_thread_by_child
            .get(child_thread_id)
            .copied()
    }

    /// Return the shared unified thread pool.
    pub fn thread_pool(&self) -> Arc<ThreadPool> {
        Arc::clone(&self.thread_pool)
    }

    pub fn set_mcp_tool_resolver(&self, resolver: Option<Arc<dyn McpToolResolver>>) {
        *self
            .mcp_tool_resolver
            .lock()
            .expect("mcp resolver mutex poisoned") = resolver;
    }

    fn current_mcp_tool_resolver(&self) -> Option<Arc<dyn McpToolResolver>> {
        self.mcp_tool_resolver
            .lock()
            .expect("mcp resolver mutex poisoned")
            .clone()
    }

    fn thread_repository(&self) -> Option<Arc<dyn ThreadRepository>> {
        self.thread_repository.clone()
    }

    fn provider_repository(&self) -> Option<Arc<dyn LlmProviderRepository>> {
        self.provider_repository.clone()
    }

    fn map_pool_error(error: ThreadPoolError) -> JobError {
        JobError::ExecutionFailed(error.to_string())
    }

    async fn resolve_provider_with_fallback(
        &self,
        provider_id: ProviderId,
        model: Option<&str>,
    ) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
        match model {
            Some(model) => match self
                .provider_resolver
                .resolve_with_model(provider_id, model)
                .await
            {
                Ok(provider) => Ok(provider),
                Err(_) => self.provider_resolver.resolve(provider_id).await,
            },
            None => self.provider_resolver.resolve(provider_id).await,
        }
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

    /// Collect the authoritative job-runtime state.
    pub fn job_runtime_state(&self) -> JobRuntimeState {
        let runtimes = self.current_job_runtime_summaries();
        let snapshot = self.collect_job_runtime_snapshot(&runtimes);
        JobRuntimeState { snapshot, runtimes }
    }

    pub fn job_runtime_summary(&self, thread_id: &ThreadId) -> Option<JobRuntimeSummary> {
        self.job_runtime_store
            .lock()
            .expect("job runtime mutex poisoned")
            .job_runtimes
            .get(thread_id)
            .cloned()
    }

    fn current_job_runtime_summaries(&self) -> Vec<JobRuntimeSummary> {
        self.job_runtime_store
            .lock()
            .expect("job runtime mutex poisoned")
            .job_runtimes
            .values()
            .cloned()
            .collect()
    }

    fn collect_job_runtime_snapshot(&self, runtimes: &[JobRuntimeSummary]) -> JobRuntimeSnapshot {
        let peak_estimated_memory_bytes = self
            .job_runtime_store
            .lock()
            .expect("job runtime mutex poisoned")
            .peak_estimated_memory_bytes;
        Self::build_job_runtime_snapshot(
            self.thread_pool.collect_metrics().max_threads,
            peak_estimated_memory_bytes,
            runtimes,
        )
    }

    fn build_job_runtime_snapshot(
        max_threads: u32,
        peak_estimated_memory_bytes: u64,
        runtimes: &[JobRuntimeSummary],
    ) -> JobRuntimeSnapshot {
        let active_threads = runtimes
            .iter()
            .filter(|runtime| {
                matches!(
                    runtime.status,
                    ThreadRuntimeStatus::Loading
                        | ThreadRuntimeStatus::Queued
                        | ThreadRuntimeStatus::Running
                        | ThreadRuntimeStatus::Cooling
                )
            })
            .count() as u32;
        let queued_threads = runtimes
            .iter()
            .filter(|runtime| runtime.status == ThreadRuntimeStatus::Queued)
            .count() as u32;
        let running_threads = runtimes
            .iter()
            .filter(|runtime| runtime.status == ThreadRuntimeStatus::Running)
            .count() as u32;
        let cooling_threads = runtimes
            .iter()
            .filter(|runtime| runtime.status == ThreadRuntimeStatus::Cooling)
            .count() as u32;
        let evicted_threads = runtimes
            .iter()
            .filter(|runtime| runtime.status == ThreadRuntimeStatus::Evicted)
            .count() as u64;
        let estimated_memory_bytes = runtimes
            .iter()
            .filter(|runtime| {
                matches!(
                    runtime.status,
                    ThreadRuntimeStatus::Loading
                        | ThreadRuntimeStatus::Queued
                        | ThreadRuntimeStatus::Running
                        | ThreadRuntimeStatus::Cooling
                )
            })
            .map(|runtime| runtime.estimated_memory_bytes)
            .sum();
        let resident_thread_count = runtimes
            .iter()
            .filter(|runtime| {
                matches!(
                    runtime.status,
                    ThreadRuntimeStatus::Loading
                        | ThreadRuntimeStatus::Queued
                        | ThreadRuntimeStatus::Running
                        | ThreadRuntimeStatus::Cooling
                )
            })
            .count() as u32;
        let avg_thread_memory_bytes = if resident_thread_count == 0 {
            0
        } else {
            estimated_memory_bytes / u64::from(resident_thread_count)
        };

        JobRuntimeSnapshot {
            max_threads,
            active_threads,
            queued_threads,
            running_threads,
            cooling_threads,
            evicted_threads,
            estimated_memory_bytes,
            peak_estimated_memory_bytes,
            process_memory_bytes: None,
            peak_process_memory_bytes: None,
            resident_thread_count,
            avg_thread_memory_bytes,
            captured_at: Utc::now().to_rfc3339(),
        }
    }

    fn refresh_job_runtime_peaks(store: &mut JobRuntimeStore) {
        let current_estimated: u64 = store
            .job_runtimes
            .values()
            .filter(|runtime| {
                matches!(
                    runtime.status,
                    ThreadRuntimeStatus::Loading
                        | ThreadRuntimeStatus::Queued
                        | ThreadRuntimeStatus::Running
                        | ThreadRuntimeStatus::Cooling
                )
            })
            .map(|runtime| runtime.estimated_memory_bytes)
            .sum();
        if current_estimated > store.peak_estimated_memory_bytes {
            store.peak_estimated_memory_bytes = current_estimated;
        }
    }

    fn merge_job_runtime_summary(
        store: &mut JobRuntimeStore,
        runtime: JobRuntimeSummary,
    ) -> JobRuntimeSummary {
        store
            .job_runtimes
            .insert(runtime.thread_id, runtime.clone());
        Self::refresh_job_runtime_peaks(store);
        runtime
    }

    fn update_job_runtime_summary_for_thread(
        store: &mut JobRuntimeStore,
        thread_id: ThreadId,
        status: ThreadRuntimeStatus,
        estimated_memory_bytes: u64,
        last_active_at: Option<String>,
        recoverable: bool,
        last_reason: Option<ThreadPoolEventReason>,
    ) -> Option<JobRuntimeSummary> {
        let runtime = store.job_runtimes.get_mut(&thread_id)?;
        runtime.status = status;
        runtime.estimated_memory_bytes = estimated_memory_bytes;
        runtime.last_active_at = last_active_at;
        runtime.recoverable = recoverable;
        runtime.last_reason = last_reason;
        let runtime = runtime.clone();
        Self::refresh_job_runtime_peaks(store);
        Some(runtime)
    }

    fn upsert_job_runtime_summary(
        &self,
        thread_id: ThreadId,
        job_id: String,
        status: ThreadRuntimeStatus,
        estimated_memory_bytes: u64,
        last_active_at: Option<String>,
        recoverable: bool,
        last_reason: Option<ThreadPoolEventReason>,
    ) -> JobRuntimeSummary {
        let mut store = self
            .job_runtime_store
            .lock()
            .expect("job runtime mutex poisoned");
        Self::merge_job_runtime_summary(
            &mut store,
            JobRuntimeSummary {
                thread_id,
                job_id,
                status,
                estimated_memory_bytes,
                last_active_at,
                recoverable,
                last_reason,
            },
        )
    }

    fn install_runtime_lifecycle_bridge(&self) {
        let thread_pool = Arc::downgrade(&self.thread_pool);
        let job_runtime_store = Arc::downgrade(&self.job_runtime_store);
        self.thread_pool
            .add_runtime_lifecycle_observer(Arc::new(move |change| {
                Self::handle_runtime_lifecycle_change(&thread_pool, &job_runtime_store, change);
            }));
    }

    fn handle_runtime_lifecycle_change(
        thread_pool: &Weak<ThreadPool>,
        job_runtime_store: &Weak<StdMutex<JobRuntimeStore>>,
        change: RuntimeLifecycleChange,
    ) {
        let Some(thread_pool) = thread_pool.upgrade() else {
            return;
        };
        let Some(job_runtime_store) = job_runtime_store.upgrade() else {
            return;
        };

        let runtime = match change {
            RuntimeLifecycleChange::Evicted(runtime) => runtime,
            RuntimeLifecycleChange::Cooling(_) => return,
        };
        let (parent_thread_id, runtime) = {
            let mut store = job_runtime_store
                .lock()
                .expect("job runtime mutex poisoned");
            let Some(runtime) = Self::update_job_runtime_summary_for_thread(
                &mut store,
                runtime.thread_id,
                runtime.status,
                runtime.estimated_memory_bytes,
                runtime.last_active_at,
                runtime.recoverable,
                runtime.last_reason.clone(),
            ) else {
                return;
            };
            let Some(parent_thread_id) = store
                .parent_thread_by_child
                .get(&runtime.thread_id)
                .copied()
            else {
                return;
            };
            (parent_thread_id, runtime)
        };

        if !thread_pool.emit_observer_event(
            &parent_thread_id,
            ThreadEvent::JobRuntimeUpdated {
                runtime: runtime.clone(),
            },
        ) {
            return;
        }
        let _ = thread_pool.emit_observer_event(
            &parent_thread_id,
            ThreadEvent::JobRuntimeEvicted {
                thread_id: runtime.thread_id,
                job_id: runtime.job_id.clone(),
                reason: runtime
                    .last_reason
                    .clone()
                    .unwrap_or(ThreadPoolEventReason::MemoryPressure),
            },
        );
        let snapshot = {
            let store = job_runtime_store
                .lock()
                .expect("job runtime mutex poisoned");
            let runtimes: Vec<_> = store.job_runtimes.values().cloned().collect();
            Self::build_job_runtime_snapshot(
                thread_pool.collect_metrics().max_threads,
                store.peak_estimated_memory_bytes,
                &runtimes,
            )
        };
        let _ = thread_pool.emit_observer_event(
            &parent_thread_id,
            ThreadEvent::JobRuntimeMetricsUpdated { snapshot },
        );
    }

    fn emit_job_runtime_updated(
        pipe_tx: &broadcast::Sender<ThreadEvent>,
        runtime: &JobRuntimeSummary,
    ) {
        let _ = pipe_tx.send(ThreadEvent::JobRuntimeUpdated {
            runtime: runtime.clone(),
        });
    }

    fn emit_job_runtime_metrics(&self, pipe_tx: &broadcast::Sender<ThreadEvent>) {
        let _ = pipe_tx.send(ThreadEvent::JobRuntimeMetricsUpdated {
            snapshot: self.job_runtime_state().snapshot,
        });
    }

    pub fn claim_delivered_job_result(
        &self,
        thread_id: ThreadId,
        job_id: &str,
    ) -> Option<MailboxMessage> {
        let mut store = self
            .job_runtime_store
            .lock()
            .expect("job runtime mutex poisoned");
        let messages = store.delivered_job_results.get_mut(&thread_id)?;
        let index = messages
            .iter()
            .position(|message| message.job_id() == Some(job_id))?;
        let claimed = messages.remove(index);
        if messages.is_empty() {
            store.delivered_job_results.remove(&thread_id);
        }
        Some(claimed)
    }

    pub async fn recover_job_execution_thread_id(
        &self,
        job_id: &str,
    ) -> Result<Option<ThreadId>, JobError> {
        if let Some(thread_id) = self.thread_binding(job_id) {
            return Ok(Some(thread_id));
        }

        if self.thread_repository.is_none() {
            return Ok(None);
        }
        let Some(job_repository) = self.job_repository.as_ref() else {
            return Ok(None);
        };
        let Some(job_record) = job_repository
            .get(&JobId::new(job_id))
            .await
            .map_err(|err| {
                JobError::ExecutionFailed(format!("failed to load job record: {err}"))
            })?
        else {
            return Ok(None);
        };
        let Some(thread_id) = job_record.thread_id else {
            return Ok(None);
        };
        self.cache_job_binding(job_id.to_string(), thread_id);

        if let Some(metadata) = self.recover_job_thread_metadata(thread_id).await? {
            self.sync_job_runtime_metadata(
                metadata.thread_id,
                metadata.job_id,
                metadata.parent_thread_id,
            );
        }

        Ok(Some(thread_id))
    }

    pub async fn recover_parent_job_thread_id(
        &self,
        child_thread_id: &ThreadId,
    ) -> Result<Option<ThreadId>, JobError> {
        if let Some(parent_thread_id) = self.parent_job_thread_id(child_thread_id) {
            return Ok(Some(parent_thread_id));
        }

        Ok(self
            .recover_job_thread_metadata(*child_thread_id)
            .await?
            .and_then(|metadata| metadata.parent_thread_id))
    }

    pub async fn recover_child_jobs_for_thread(
        &self,
        parent_thread_id: ThreadId,
    ) -> Result<Vec<RecoveredChildJob>, JobError> {
        if let Some(children) = self
            .job_runtime_store
            .lock()
            .expect("job runtime mutex poisoned")
            .child_jobs_by_parent
            .get(&parent_thread_id)
            .cloned()
        {
            return Ok(children);
        }

        let parent_base_dir = self.trace_base_dir_for_thread(parent_thread_id).await?;
        let metadata = list_direct_child_threads(&parent_base_dir, parent_thread_id)
            .await
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        let mut children = Vec::with_capacity(metadata.len());
        for child_metadata in metadata {
            let job_id = child_metadata.job_id.clone().ok_or_else(|| {
                JobError::ExecutionFailed(format!(
                    "job thread {} is missing persisted job_id metadata",
                    child_metadata.thread_id
                ))
            })?;
            self.sync_job_runtime_metadata(
                child_metadata.thread_id,
                child_metadata.job_id.clone(),
                child_metadata.parent_thread_id,
            );
            children.push(RecoveredChildJob {
                thread_id: child_metadata.thread_id,
                job_id,
            });
        }
        {
            let mut store = self
                .job_runtime_store
                .lock()
                .expect("job runtime mutex poisoned");
            for child in &children {
                store
                    .job_bindings
                    .insert(child.job_id.clone(), child.thread_id);
                store
                    .parent_thread_by_child
                    .insert(child.thread_id, parent_thread_id);
            }
            store
                .child_jobs_by_parent
                .insert(parent_thread_id, children.clone());
        }
        Ok(children)
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
            .recover_job_thread_metadata(execution_thread_id)
            .await?
        else {
            return Ok(JobLookup::NotFound);
        };
        self.sync_job_runtime_metadata(
            metadata.thread_id,
            metadata.job_id.clone(),
            metadata.parent_thread_id,
        );
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
                    cancelled: matches!(job_record.status, JobStatus::Cancelled),
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

        let request = JobExecutionRequest {
            originating_thread_id,
            job_id: job_id.clone(),
            agent_id,
            prompt,
            context,
        };

        let execution_thread_id = self.enqueue_job_runtime(&request).await?;

        let cancellation = TurnCancellation::new();
        let spawn_cancellation = cancellation.clone();
        let manager = self.clone();
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
        if let Some(runtime) = self.job_runtime_summary(&execution_thread_id) {
            Self::emit_job_runtime_updated(&pipe_tx, &runtime);
        }
        let _ = pipe_tx.send(ThreadEvent::JobRuntimeQueued {
            thread_id: execution_thread_id,
            job_id: job_id.clone(),
        });
        self.emit_job_runtime_metrics(&pipe_tx);

        let pipe_tx_clone = pipe_tx.clone();

        tokio::spawn(async move {
            let result = manager
                .execute_job_runtime(
                    request,
                    execution_thread_id,
                    pipe_tx_clone.clone(),
                    spawn_cancellation,
                )
                .await;

            manager
                .forward_job_result_to_runtime(
                    originating_thread_id,
                    execution_thread_id,
                    result.clone(),
                )
                .await;
            Self::record_completed_job_result_in_store(
                &manager.tracked_jobs,
                originating_thread_id,
                result.clone(),
            );
            Self::broadcast_job_result(&pipe_tx_clone, originating_thread_id, result);
        });

        Ok(())
    }

    async fn enqueue_job_runtime(
        &self,
        request: &JobExecutionRequest,
    ) -> Result<ThreadId, JobError> {
        let now = Utc::now().to_rfc3339();
        let thread_id = self.persist_binding(request, &now).await?;
        self.persist_job_status(&request.job_id, JobStatus::Queued, None, None)
            .await?;
        self.thread_pool.register_runtime(
            thread_id,
            ThreadRuntimeStatus::Queued,
            request.prompt.len() as u64,
            Some(now.clone()),
            true,
            None,
            None,
        );
        self.upsert_job_runtime_summary(
            thread_id,
            request.job_id.clone(),
            ThreadRuntimeStatus::Queued,
            request.prompt.len() as u64,
            Some(now),
            true,
            None,
        );
        self.sync_job_runtime_metadata(
            thread_id,
            Some(request.job_id.clone()),
            Some(request.originating_thread_id),
        );
        Ok(thread_id)
    }

    async fn execute_job_runtime(
        &self,
        request: JobExecutionRequest,
        execution_thread_id: ThreadId,
        pipe_tx: broadcast::Sender<ThreadEvent>,
        cancellation: TurnCancellation,
    ) -> ThreadJobResult {
        let fallback_job_id = request.job_id.clone();
        let fallback_agent_id = request.agent_id;
        let fallback_display_name = format!("Agent {}", fallback_agent_id.inner());
        let thread = match self
            .ensure_job_runtime(&request, execution_thread_id, &pipe_tx)
            .await
        {
            Ok(thread) => thread,
            Err(error) => {
                let result = Self::failure_result(
                    fallback_job_id,
                    fallback_agent_id,
                    fallback_display_name,
                    String::new(),
                    error.to_string(),
                );
                self.persist_job_completion(&request.job_id, &result, None)
                    .await;
                return result;
            }
        };
        let runtime_rx = match self.thread_pool.subscribe(&execution_thread_id) {
            Some(rx) => rx,
            None => {
                let result = Self::failure_result(
                    fallback_job_id,
                    fallback_agent_id,
                    fallback_display_name,
                    String::new(),
                    format!(
                        "job runtime {} is missing a runtime event stream",
                        execution_thread_id
                    ),
                );
                self.persist_job_completion(&request.job_id, &result, None)
                    .await;
                return result;
            }
        };
        let started_at = Utc::now().to_rfc3339();
        let estimated_memory_bytes =
            ThreadPool::estimate_thread_memory(&thread) + request.prompt.len() as u64;
        self.thread_pool.mark_runtime_running(
            &execution_thread_id,
            estimated_memory_bytes,
            started_at.clone(),
        );
        if let Err(error) = self
            .persist_job_status(
                &request.job_id,
                JobStatus::Running,
                Some(started_at.as_str()),
                None,
            )
            .await
        {
            tracing::warn!(
                job_id = %request.job_id,
                error = %error,
                "Failed to persist running job status"
            );
        }
        let runtime = self.upsert_job_runtime_summary(
            execution_thread_id,
            request.job_id.clone(),
            ThreadRuntimeStatus::Running,
            estimated_memory_bytes,
            Some(started_at.clone()),
            true,
            None,
        );
        Self::emit_job_runtime_updated(&pipe_tx, &runtime);
        let _ = pipe_tx.send(ThreadEvent::JobRuntimeStarted {
            thread_id: execution_thread_id,
            job_id: request.job_id.clone(),
        });
        self.emit_job_runtime_metrics(&pipe_tx);

        let cancellation_for_wait = cancellation.clone();

        let result = if request.prompt == "__panic_thread_pool_execute_turn__" {
            Self::failure_result(
                fallback_job_id.clone(),
                fallback_agent_id,
                fallback_display_name,
                String::new(),
                "job executor panicked: thread pool panic test hook".to_string(),
            )
        } else {
            let task_assignment = MailboxMessage {
                id: Uuid::new_v4().to_string(),
                from_thread_id: request.originating_thread_id,
                to_thread_id: execution_thread_id,
                from_label: self
                    .thread_display_label(&request.originating_thread_id)
                    .await,
                message_type: MailboxMessageType::TaskAssignment {
                    task_id: request.job_id.clone(),
                    subject: Self::task_subject(&request.prompt),
                    description: request.prompt.clone(),
                },
                text: request.prompt.clone(),
                timestamp: started_at.clone(),
                read: false,
                summary: request.context.as_ref().map(|context| context.to_string()),
            };

            if cancellation_for_wait.is_cancelled() {
                Self::cancelled_result(
                    fallback_job_id,
                    fallback_agent_id,
                    fallback_display_name,
                    String::new(),
                    "Turn cancelled".to_string(),
                )
            } else {
                match self
                    .thread_pool
                    .deliver_thread_message(
                        execution_thread_id,
                        Self::route_mailbox_message(task_assignment),
                    )
                    .await
                {
                    Ok(()) => {
                        let cancellation_thread = thread.clone();
                        let cancellation_signal = cancellation.clone();
                        let cancellation_forwarder = tokio::spawn(async move {
                            cancellation_signal.cancelled().await;
                            let _ = cancellation_thread.send_message(ThreadMessage::Interrupt);
                        });

                        let result = self
                            .await_job_turn_result(
                                execution_thread_id,
                                &thread,
                                runtime_rx,
                                request.job_id.clone(),
                                cancellation_for_wait,
                            )
                            .await;
                        cancellation_forwarder.abort();
                        result
                    }
                    Err(error) => Self::failure_result(
                        fallback_job_id,
                        fallback_agent_id,
                        fallback_display_name,
                        String::new(),
                        error.to_string(),
                    ),
                }
            }
        };
        self.persist_thread_stats(&execution_thread_id, &thread)
            .await;
        self.persist_job_completion(&request.job_id, &result, Some(started_at.as_str()))
            .await;

        let cooling_memory = ThreadPool::estimate_thread_memory(&thread);
        let terminal_reason = if result.cancelled {
            Some(ThreadPoolEventReason::Cancelled)
        } else if result.success {
            None
        } else {
            Some(ThreadPoolEventReason::ExecutionFailed)
        };

        if self
            .thread_pool
            .transition_runtime_to_cooling(&execution_thread_id, Some(cooling_memory))
            .is_some()
        {
            let runtime = self.upsert_job_runtime_summary(
                execution_thread_id,
                request.job_id.clone(),
                ThreadRuntimeStatus::Cooling,
                cooling_memory,
                Some(Utc::now().to_rfc3339()),
                true,
                terminal_reason,
            );
            Self::emit_job_runtime_updated(&pipe_tx, &runtime);
            let _ = pipe_tx.send(ThreadEvent::JobRuntimeCooling {
                thread_id: execution_thread_id,
                job_id: request.job_id.clone(),
            });
            self.emit_job_runtime_metrics(&pipe_tx);
        }

        result
    }

    async fn ensure_job_runtime(
        &self,
        request: &JobExecutionRequest,
        thread_id: ThreadId,
        pipe_tx: &broadcast::Sender<ThreadEvent>,
    ) -> Result<ThreadHandle, JobError> {
        let runtime = self.upsert_job_runtime_summary(
            thread_id,
            request.job_id.clone(),
            ThreadRuntimeStatus::Loading,
            0,
            Some(Utc::now().to_rfc3339()),
            true,
            None,
        );
        Self::emit_job_runtime_updated(pipe_tx, &runtime);
        self.emit_job_runtime_metrics(pipe_tx);

        let manager = self.clone();
        let request_for_build = request.clone();
        let job_id = request.job_id.clone();
        let thread = if let Some(thread) = self.thread_pool.loaded_runtime(&thread_id) {
            thread
        } else {
            match self
                .thread_pool
                .load_runtime_with_builder(thread_id, "job thread", false, None, true, move || {
                    let manager = manager.clone();
                    let request = request_for_build.clone();
                    async move {
                        manager
                            .build_job_thread(&request, thread_id)
                            .await
                            .map_err(|error| ThreadPoolError::ExecutionFailed(error.to_string()))
                    }
                })
                .await
            {
                Ok(thread) => thread,
                Err(error) => {
                    let error = Self::map_pool_error(error);
                    let runtime = self.upsert_job_runtime_summary(
                        thread_id,
                        job_id,
                        ThreadRuntimeStatus::Inactive,
                        0,
                        Some(Utc::now().to_rfc3339()),
                        true,
                        Some(ThreadPoolEventReason::ExecutionFailed),
                    );
                    Self::emit_job_runtime_updated(pipe_tx, &runtime);
                    self.emit_job_runtime_metrics(pipe_tx);
                    return Err(error);
                }
            }
        };
        Ok(thread)
    }

    async fn build_job_thread(
        &self,
        request: &JobExecutionRequest,
        thread_id: ThreadId,
    ) -> Result<Thread, JobError> {
        let thread_record = if let Some(thread_repository) = self.thread_repository() {
            thread_repository
                .get_thread(&thread_id)
                .await
                .map_err(|err| {
                    JobError::ExecutionFailed(format!("failed to load thread record: {err}"))
                })?
        } else {
            None
        };
        let base_dir = find_job_thread_base_dir(&self.trace_dir, thread_id)
            .await
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        let metadata = recover_and_validate_metadata(&base_dir, thread_id, ThreadTraceKind::Job)
            .await
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        if metadata.parent_thread_id != Some(request.originating_thread_id) {
            return Err(JobError::ExecutionFailed(format!(
                "job thread {} is bound to parent {:?}, not {}",
                thread_id, metadata.parent_thread_id, request.originating_thread_id
            )));
        }
        if metadata.job_id.as_deref() != Some(request.job_id.as_str()) {
            return Err(JobError::ExecutionFailed(format!(
                "job thread {} is bound to job {:?}, not {}",
                thread_id, metadata.job_id, request.job_id
            )));
        }
        let agent_record = metadata.agent_snapshot.clone();
        let provider = if let Some(thread_record) = thread_record.as_ref() {
            let provider_id = ProviderId::new(thread_record.provider_id.into_inner());
            self.resolve_provider_with_fallback(
                provider_id,
                thread_record.model_override.as_deref(),
            )
            .await
        } else if let Some(provider_id) = agent_record.provider_id {
            self.resolve_provider_with_fallback(provider_id, agent_record.model_id.as_deref())
                .await
        } else {
            self.provider_resolver.default_provider().await
        }
        .map_err(|err| JobError::ExecutionFailed(format!("failed to resolve provider: {err}")))?;

        let config = build_thread_config(base_dir.clone(), provider.model_name().to_string())
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        let plan_store = FilePlanStore::new(base_dir.clone());
        let thread_title = thread_record
            .as_ref()
            .and_then(|record| record.title.clone())
            .or_else(|| Some(format!("job:{}", request.job_id)));
        let mut builder = ThreadBuilder::new()
            .id(thread_id)
            .session_id(Self::job_runtime_session_id(thread_id))
            .agent_record(Arc::new(agent_record))
            .title(thread_title)
            .provider(provider.clone())
            .tool_manager(Arc::clone(&self.tool_manager))
            .compactor(Arc::new(LlmThreadCompactor::new(provider)))
            .plan_store(plan_store)
            .config(config);
        if let Some(resolver) = self.current_mcp_tool_resolver() {
            builder = builder.mcp_tool_resolver(resolver);
        }
        let mut thread = builder
            .build()
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        self.sync_job_runtime_metadata(
            metadata.thread_id,
            metadata.job_id.clone(),
            metadata.parent_thread_id,
        );

        if let Some(thread_record) = thread_record {
            hydrate_turn_log_state(&mut thread, &base_dir, &thread_record.updated_at)
                .await
                .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        }

        Ok(thread)
    }

    fn job_runtime_session_id(thread_id: ThreadId) -> SessionId {
        SessionId(*thread_id.inner())
    }

    async fn persist_thread_stats(&self, thread_id: &ThreadId, thread: &ThreadHandle) {
        let Some(thread_repository) = self.thread_repository() else {
            return;
        };
        let token_count = thread.token_count();
        let turn_count = thread.turn_count();
        if let Err(error) = thread_repository
            .update_thread_stats(thread_id, token_count, turn_count)
            .await
        {
            tracing::warn!(
                thread_id = %thread_id,
                error = %error,
                "Failed to persist job thread stats"
            );
        }
    }

    async fn persist_job_status(
        &self,
        job_id: &str,
        status: JobStatus,
        started_at: Option<&str>,
        finished_at: Option<&str>,
    ) -> Result<(), JobError> {
        let Some(job_repository) = self.job_repository.as_ref() else {
            return Ok(());
        };
        job_repository
            .update_status(&JobId::new(job_id), status, started_at, finished_at)
            .await
            .map_err(|err| {
                JobError::ExecutionFailed(format!("failed to persist job status: {err}"))
            })
    }

    async fn persist_job_completion(
        &self,
        job_id: &str,
        result: &ThreadJobResult,
        started_at: Option<&str>,
    ) {
        let Some(job_repository) = self.job_repository.as_ref() else {
            return;
        };
        let persisted_result = JobResult {
            success: result.success,
            message: result.message.clone(),
            token_usage: result.token_usage.clone(),
            agent_id: RepoAgentId::new(result.agent_id.inner()),
            agent_display_name: result.agent_display_name.clone(),
            agent_description: result.agent_description.clone(),
        };
        if let Err(error) = job_repository
            .update_result(&JobId::new(job_id), &persisted_result)
            .await
        {
            tracing::warn!(
                job_id,
                error = %error,
                "Failed to persist job result"
            );
            return;
        }

        let finished_at = Utc::now().to_rfc3339();
        let status = if result.success {
            JobStatus::Succeeded
        } else if result.cancelled {
            JobStatus::Cancelled
        } else {
            JobStatus::Failed
        };
        if let Err(error) = job_repository
            .update_status(
                &JobId::new(job_id),
                status,
                started_at,
                Some(finished_at.as_str()),
            )
            .await
        {
            tracing::warn!(
                job_id,
                error = %error,
                "Failed to persist final job status"
            );
        }
    }

    async fn persist_binding(
        &self,
        request: &JobExecutionRequest,
        now: &str,
    ) -> Result<ThreadId, JobError> {
        if self.thread_repository.is_none()
            || self.provider_repository.is_none()
            || self.job_repository.is_none()
        {
            return Ok(ThreadId::new());
        }

        let agent_record = self
            .template_manager
            .get(request.agent_id)
            .await
            .map_err(|err| {
                JobError::ExecutionFailed(format!(
                    "failed to load agent {}: {err}",
                    request.agent_id.inner()
                ))
            })?;
        let agent_record = agent_record.ok_or_else(|| {
            JobError::ExecutionFailed(format!("agent {} not found", request.agent_id.inner()))
        })?;
        let parent_base_dir = self
            .trace_base_dir_for_thread(request.originating_thread_id)
            .await?;
        let parent_metadata = recover_thread_metadata(&parent_base_dir)
            .await
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;

        let Some(thread_repository) = self.thread_repository() else {
            return Ok(ThreadId::new());
        };
        let Some(provider_repository) = self.provider_repository() else {
            return Ok(ThreadId::new());
        };
        let Some(job_repository) = self.job_repository.as_ref() else {
            return Ok(ThreadId::new());
        };

        let job_id = JobId::new(request.job_id.clone());
        let existing_job = job_repository.get(&job_id).await.map_err(|err| {
            JobError::ExecutionFailed(format!("failed to load job record: {err}"))
        })?;
        let existing_thread_id = existing_job.as_ref().and_then(|job| job.thread_id);
        let existing_thread = if let Some(thread_id) = existing_thread_id {
            thread_repository
                .get_thread(&thread_id)
                .await
                .map_err(|err| {
                    JobError::ExecutionFailed(format!("failed to load thread record: {err}"))
                })?
        } else {
            None
        };

        let thread_id = existing_thread_id.unwrap_or_else(ThreadId::new);
        let should_cleanup_trace_dir = existing_thread_id.is_none();
        let default_base_dir = child_thread_base_dir(&parent_base_dir, thread_id);
        let base_dir = if existing_thread_id.is_some() {
            let existing_base_dir = find_job_thread_base_dir(&self.trace_dir, thread_id)
                .await
                .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
            if existing_base_dir != default_base_dir {
                return Err(JobError::ExecutionFailed(format!(
                    "job thread {} cannot move between parents without trace migration",
                    thread_id
                )));
            }
            let metadata = recover_thread_metadata(&existing_base_dir)
                .await
                .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
            if metadata.parent_thread_id != Some(request.originating_thread_id) {
                return Err(JobError::ExecutionFailed(format!(
                    "job thread {} is already bound to parent {:?}",
                    thread_id, metadata.parent_thread_id
                )));
            }
            if metadata.job_id.as_deref() != Some(request.job_id.as_str()) {
                return Err(JobError::ExecutionFailed(format!(
                    "job thread {} is already bound to job {:?}",
                    thread_id, metadata.job_id
                )));
            }
            existing_base_dir
        } else {
            default_base_dir
        };

        let metadata = ThreadTraceMetadata {
            thread_id,
            kind: ThreadTraceKind::Job,
            root_session_id: parent_metadata.root_session_id,
            parent_thread_id: Some(request.originating_thread_id),
            job_id: Some(request.job_id.clone()),
            agent_snapshot: agent_record.clone(),
        };
        persist_thread_metadata(&base_dir, &metadata)
            .await
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        self.sync_job_runtime_metadata(
            metadata.thread_id,
            metadata.job_id.clone(),
            metadata.parent_thread_id,
        );

        let template_provider_id = agent_record
            .provider_id
            .map(|id| argus_protocol::LlmProviderId::new(id.inner()));
        let provider_id = match template_provider_id {
            Some(provider_id) => provider_id,
            None => provider_repository
                .get_default_provider_id()
                .await
                .map_err(|err| {
                    JobError::ExecutionFailed(format!(
                        "failed to resolve default provider id: {err}"
                    ))
                })?
                .ok_or_else(|| {
                    JobError::ExecutionFailed("default provider is not configured".to_string())
                })?,
        };
        let model_override = agent_record.model_id.clone();

        let mut thread_record = existing_thread.unwrap_or(ThreadRecord {
            id: thread_id,
            provider_id,
            title: Some(format!("job:{}", request.job_id)),
            token_count: 0,
            turn_count: 0,
            session_id: None,
            template_id: Some(RepoAgentId::new(request.agent_id.inner())),
            model_override: model_override.clone(),
            created_at: now.to_string(),
            updated_at: now.to_string(),
        });
        thread_record.id = thread_id;
        thread_record.provider_id = provider_id;
        thread_record.title = Some(format!("job:{}", request.job_id));
        thread_record.session_id = None;
        thread_record.template_id = Some(RepoAgentId::new(request.agent_id.inner()));
        thread_record.model_override = model_override;
        thread_record.updated_at = now.to_string();
        if let Err(err) = thread_repository.upsert_thread(&thread_record).await {
            if should_cleanup_trace_dir {
                cleanup_trace_dir(&base_dir).await;
            }
            return Err(JobError::ExecutionFailed(format!(
                "failed to persist thread record: {err}"
            )));
        }

        if existing_job.is_some() {
            if existing_thread_id.is_none()
                && let Err(err) =
                    Self::persist_existing_job_binding(job_repository, &job_id, thread_id).await
            {
                if should_cleanup_trace_dir {
                    cleanup_trace_dir(&base_dir).await;
                }
                return Err(Self::rollback_thread_record(
                    &thread_repository,
                    thread_id,
                    format!("{err}"),
                )
                .await);
            }
            return Ok(thread_id);
        }

        let job_record = JobRecord {
            id: job_id,
            job_type: JobType::Standalone,
            name: format!("job:{}", request.job_id),
            status: JobStatus::Pending,
            agent_id: RepoAgentId::new(request.agent_id.inner()),
            context: request
                .context
                .as_ref()
                .map(std::string::ToString::to_string),
            prompt: request.prompt.clone(),
            thread_id: Some(thread_id),
            group_id: None,
            depends_on: Vec::new(),
            cron_expr: None,
            scheduled_at: None,
            started_at: None,
            finished_at: None,
            parent_job_id: None,
            result: None,
        };

        if let Err(err) = job_repository.create(&job_record).await {
            if should_cleanup_trace_dir {
                cleanup_trace_dir(&base_dir).await;
            }
            return Err(Self::rollback_thread_record(
                &thread_repository,
                thread_id,
                format!("failed to create job record: {err}"),
            )
            .await);
        }

        Ok(thread_id)
    }

    async fn persist_existing_job_binding(
        job_repository: &Arc<dyn JobRepository>,
        job_id: &JobId,
        thread_id: ThreadId,
    ) -> Result<(), JobError> {
        job_repository
            .update_thread_id(job_id, &thread_id)
            .await
            .map_err(|err| {
                JobError::ExecutionFailed(format!("failed to persist job-thread binding: {err}"))
            })
    }

    async fn rollback_thread_record(
        thread_repository: &Arc<dyn ThreadRepository>,
        thread_id: ThreadId,
        message: String,
    ) -> JobError {
        match thread_repository.delete_thread(&thread_id).await {
            Ok(_) => JobError::ExecutionFailed(message),
            Err(cleanup_err) => JobError::ExecutionFailed(format!(
                "{message}; failed to roll back thread record: {cleanup_err}"
            )),
        }
    }

    async fn recover_job_thread_metadata(
        &self,
        thread_id: ThreadId,
    ) -> Result<Option<ThreadTraceMetadata>, JobError> {
        let base_dir = match find_job_thread_base_dir(&self.trace_dir, thread_id).await {
            Ok(base_dir) => base_dir,
            Err(argus_agent::error::TurnLogError::ThreadMetadataNotFound(_)) => {
                return Ok(None);
            }
            Err(error) => {
                return Err(JobError::ExecutionFailed(format!(
                    "failed to locate job trace metadata: {error}"
                )));
            }
        };
        let metadata = recover_and_validate_metadata(&base_dir, thread_id, ThreadTraceKind::Job)
            .await
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        self.sync_job_runtime_metadata(
            metadata.thread_id,
            metadata.job_id.clone(),
            metadata.parent_thread_id,
        );
        Ok(Some(metadata))
    }

    async fn trace_base_dir_for_thread(&self, thread_id: ThreadId) -> Result<PathBuf, JobError> {
        if let Some(thread) = self.thread_pool.loaded_thread(&thread_id) {
            return thread.trace_base_dir().ok_or_else(|| {
                JobError::ExecutionFailed(format!(
                    "thread {} does not expose a trace directory",
                    thread_id
                ))
            });
        }

        if let Some(thread_repository) = self.thread_repository()
            && let Some(thread_record) =
                thread_repository
                    .get_thread(&thread_id)
                    .await
                    .map_err(|err| {
                        JobError::ExecutionFailed(format!("failed to load thread record: {err}"))
                    })?
            && let Some(session_id) = thread_record.session_id
        {
            return Ok(argus_agent::thread_trace_store::chat_thread_base_dir(
                &self.trace_dir,
                session_id,
                thread_id,
            ));
        }

        find_job_thread_base_dir(&self.trace_dir, thread_id)
            .await
            .map_err(|_| {
                JobError::ExecutionFailed(format!("thread {} trace directory not found", thread_id))
            })
    }

    async fn thread_display_label(&self, thread_id: &ThreadId) -> String {
        let Some(thread) = self.thread_pool.loaded_thread(thread_id) else {
            return format!("Thread {}", thread_id);
        };

        thread.agent_display_name()
    }

    fn route_mailbox_message(message: MailboxMessage) -> ThreadMessage {
        if matches!(message.message_type, MailboxMessageType::JobResult { .. }) {
            ThreadMessage::JobResult { message }
        } else {
            ThreadMessage::PeerMessage { message }
        }
    }

    fn task_subject(prompt: &str) -> String {
        let subject = prompt
            .lines()
            .find(|line| !line.trim().is_empty())
            .map(str::trim)
            .unwrap_or("Task");
        const SUBJECT_LIMIT: usize = 120;
        let mut chars = subject.chars();
        let subject: String = chars.by_ref().take(SUBJECT_LIMIT).collect();
        if chars.next().is_some() {
            format!("{subject}...")
        } else {
            subject
        }
    }

    async fn summarize_thread_history(thread: &ThreadHandle) -> String {
        const SUMMARY_LIMIT: usize = 4000;

        let summary = thread
            .history()
            .into_iter()
            .rev()
            .find_map(|message| match message {
                ChatMessage {
                    role: Role::Assistant,
                    content,
                    ..
                } if !content.is_empty() => Some(content),
                _ => None,
            });

        match summary {
            Some(content) => {
                let mut chars = content.chars();
                let summary: String = chars.by_ref().take(SUMMARY_LIMIT).collect();
                if chars.next().is_some() {
                    format!("{summary}...")
                } else {
                    content
                }
            }
            None => "job completed".to_string(),
        }
    }

    async fn await_job_turn_result(
        &self,
        execution_thread_id: ThreadId,
        thread: &ThreadHandle,
        mut runtime_rx: broadcast::Receiver<ThreadEvent>,
        fallback_job_id: String,
        cancellation: TurnCancellation,
    ) -> ThreadJobResult {
        let agent_id = thread.agent_id();
        let agent_display_name = thread.agent_display_name();
        let agent_description = thread.agent_description();

        let mut token_usage = None;
        let mut failure = None;
        let thread_id_str = execution_thread_id.inner().to_string();
        let mut terminal_turn_number = None;

        loop {
            match runtime_rx.recv().await {
                Ok(ThreadEvent::TurnCompleted {
                    thread_id,
                    turn_number,
                    token_usage: completed_usage,
                    ..
                }) if thread_id == thread_id_str => {
                    token_usage = Some(completed_usage);
                    terminal_turn_number = Some(turn_number);
                }
                Ok(ThreadEvent::TurnFailed {
                    thread_id,
                    turn_number,
                    error,
                }) if thread_id == thread_id_str => {
                    failure = Some(error);
                    terminal_turn_number = Some(turn_number);
                }
                Ok(ThreadEvent::TurnSettled {
                    thread_id,
                    turn_number,
                }) if thread_id == thread_id_str => {
                    if terminal_turn_number == Some(turn_number) {
                        break;
                    }
                }
                Ok(ThreadEvent::Idle { thread_id }) if thread_id == thread_id_str => {
                    if terminal_turn_number.is_some() {
                        continue;
                    }

                    let message = if cancellation.is_cancelled() {
                        "Turn cancelled".to_string()
                    } else {
                        "job runtime became idle without a terminal turn result".to_string()
                    };
                    return if cancellation.is_cancelled() {
                        Self::cancelled_result(
                            fallback_job_id,
                            agent_id,
                            agent_display_name,
                            agent_description,
                            message,
                        )
                    } else {
                        Self::failure_result(
                            fallback_job_id,
                            agent_id,
                            agent_display_name,
                            agent_description,
                            message,
                        )
                    };
                }
                Ok(_) => {}
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
                Err(broadcast::error::RecvError::Closed) => {
                    return Self::failure_result(
                        fallback_job_id,
                        agent_id,
                        agent_display_name,
                        agent_description,
                        "job runtime event stream closed unexpectedly".to_string(),
                    );
                }
            }
        }

        if let Some(message) = failure {
            return if cancellation.is_cancelled() {
                Self::cancelled_result(
                    fallback_job_id,
                    agent_id,
                    agent_display_name,
                    agent_description,
                    message,
                )
            } else {
                Self::failure_result(
                    fallback_job_id,
                    agent_id,
                    agent_display_name,
                    agent_description,
                    message,
                )
            };
        }

        ThreadJobResult {
            job_id: fallback_job_id,
            success: true,
            cancelled: false,
            message: Self::summarize_thread_history(thread).await,
            token_usage,
            agent_id,
            agent_display_name,
            agent_description,
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
            cancelled: false,
            message,
            token_usage: None,
            agent_id,
            agent_display_name,
            agent_description,
        }
    }

    fn cancelled_result(
        job_id: String,
        agent_id: AgentId,
        agent_display_name: String,
        agent_description: String,
        message: String,
    ) -> ThreadJobResult {
        ThreadJobResult {
            job_id,
            success: false,
            cancelled: true,
            message,
            token_usage: None,
            agent_id,
            agent_display_name,
            agent_description,
        }
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
        let Some(thread_id) = self.thread_binding(job_id) else {
            return true;
        };

        self.job_runtime_summary(&thread_id).is_some_and(|runtime| {
            matches!(
                runtime.status,
                ThreadRuntimeStatus::Loading
                    | ThreadRuntimeStatus::Queued
                    | ThreadRuntimeStatus::Running
            )
        })
    }

    async fn forward_job_result_to_runtime(
        &self,
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
                cancelled: result.cancelled,
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
        self.record_delivered_job_result(originating_thread_id, mailbox_message.clone());
        let forwarder = self
            .chat_mailbox_forwarder
            .lock()
            .expect("chat mailbox forwarder mutex poisoned")
            .clone();
        let forwarded = match forwarder {
            Some(forwarder) => forwarder(originating_thread_id, mailbox_message.clone()).await,
            None => false,
        };
        if !forwarded {
            let _ = self
                .thread_pool
                .deliver_thread_message(
                    originating_thread_id,
                    Self::route_mailbox_message(mailbox_message),
                )
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
            cancelled: result.cancelled,
            message: result.message,
            token_usage: result.token_usage,
            agent_id: result.agent_id,
            agent_display_name: result.agent_display_name,
            agent_description: result.agent_description,
        });
    }

    fn cache_job_binding(&self, job_id: String, thread_id: ThreadId) {
        self.job_runtime_store
            .lock()
            .expect("job runtime mutex poisoned")
            .job_bindings
            .insert(job_id, thread_id);
    }

    fn cache_parent_job_thread(
        &self,
        child_thread_id: ThreadId,
        parent_thread_id: ThreadId,
        job_id: Option<String>,
    ) {
        let mut store = self
            .job_runtime_store
            .lock()
            .expect("job runtime mutex poisoned");
        store
            .parent_thread_by_child
            .insert(child_thread_id, parent_thread_id);
        let children = store
            .child_jobs_by_parent
            .entry(parent_thread_id)
            .or_default();
        if let Some(existing) = children
            .iter_mut()
            .find(|child| child.thread_id == child_thread_id)
        {
            if let Some(job_id) = job_id {
                existing.job_id = job_id;
            }
            return;
        }
        children.push(RecoveredChildJob {
            thread_id: child_thread_id,
            job_id: job_id.unwrap_or_default(),
        });
    }

    fn sync_job_runtime_metadata(
        &self,
        thread_id: ThreadId,
        job_id: Option<String>,
        parent_thread_id: Option<ThreadId>,
    ) {
        if let Some(job_id) = job_id.clone() {
            self.cache_job_binding(job_id.clone(), thread_id);
            if !self
                .job_runtime_store
                .lock()
                .expect("job runtime mutex poisoned")
                .job_runtimes
                .contains_key(&thread_id)
            {
                self.upsert_job_runtime_summary(
                    thread_id,
                    job_id,
                    ThreadRuntimeStatus::Inactive,
                    0,
                    None,
                    true,
                    None,
                );
            }
        }
        if let Some(parent_thread_id) = parent_thread_id {
            self.cache_parent_job_thread(thread_id, parent_thread_id, job_id);
        }
    }

    fn record_delivered_job_result(&self, thread_id: ThreadId, message: MailboxMessage) {
        self.job_runtime_store
            .lock()
            .expect("job runtime mutex poisoned")
            .delivered_job_results
            .entry(thread_id)
            .or_default()
            .push(message);
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
        AgentRecord, LlmProvider, ProviderId, SessionId, ThinkingConfig, ThreadId,
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
        let thread_pool = Arc::new(ThreadPool::new());
        JobManager::new(
            thread_pool,
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
            subagent_names: vec![],
            max_tokens: None,
            temperature: None,
            thinking_config: Some(ThinkingConfig::enabled()),
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
                Arc::new(ThreadPool::new()),
                template_manager,
                Arc::new(FixedProviderResolver::new(provider)),
                Arc::new(ToolManager::new()),
                trace_dir,
                Some(sqlite.clone() as Arc<dyn JobRepository>),
                Some(sqlite.clone() as Arc<dyn ThreadRepository>),
                Some(sqlite as Arc<dyn LlmProviderRepository>),
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
            Arc::new(ThreadPool::new()),
            Arc::new(TemplateManager::new(
                sqlite.clone() as Arc<dyn AgentRepository>,
                sqlite.clone(),
            )),
            Arc::new(DummyProviderResolver),
            Arc::new(ToolManager::new()),
            std::env::temp_dir().join("argus-job-tests"),
            Some(sqlite.clone() as Arc<dyn JobRepository>),
            Some(sqlite.clone() as Arc<dyn ThreadRepository>),
            Some(sqlite as Arc<dyn LlmProviderRepository>),
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
            cancelled: false,
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
            .job_runtime_state()
            .runtimes
            .into_iter()
            .find(|runtime| runtime.thread_id == bound_thread_id)
            .expect("bound runtime should be tracked in job runtime state");
        assert_eq!(runtime.job_id, job_id);
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
        let mut saw_failure_update = false;
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
                    Ok(ThreadEvent::JobRuntimeQueued {
                        thread_id,
                        job_id: event_job_id,
                    }) if event_job_id == job_id => {
                        if let Some(execution_thread_id) = bound_thread_id {
                            assert_eq!(thread_id, execution_thread_id);
                        }
                        saw_queued = true;
                    }
                    Ok(ThreadEvent::JobRuntimeMetricsUpdated { .. }) => {
                        saw_metrics = true;
                    }
                    Ok(ThreadEvent::JobRuntimeUpdated { runtime })
                        if runtime.job_id == job_id
                            && runtime.status == ThreadRuntimeStatus::Inactive
                            && runtime.last_reason == Some(ThreadPoolEventReason::ExecutionFailed) =>
                    {
                        saw_failure_update = true;
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
        assert!(
            saw_failure_update,
            "load failure should publish a runtime update"
        );
        assert!(saw_metrics, "metrics update should be observed");
    }

    #[tokio::test]
    async fn cooling_job_eviction_publishes_job_runtime_events_through_parent_thread() {
        let provider = Arc::new(CapturingProvider::new(
            "done",
            Duration::from_millis(10),
            24,
        ));
        let (manager, agent_id, originating_thread_id) =
            test_job_manager_with_provider(provider).await;
        manager.thread_pool().register_runtime(
            originating_thread_id,
            ThreadRuntimeStatus::Inactive,
            0,
            None,
            true,
            None,
            None,
        );
        let mut parent_rx = manager
            .thread_pool()
            .subscribe(&originating_thread_id)
            .expect("parent runtime should be registered");
        let (pipe_tx, _pipe_rx) = broadcast::channel(32);
        let job_id = "job-eviction-bridge".to_string();

        manager
            .dispatch_job(
                originating_thread_id,
                job_id.clone(),
                agent_id,
                "finish quickly".to_string(),
                None,
                pipe_tx,
            )
            .await
            .expect("dispatch should succeed");

        let execution_thread_id = timeout(Duration::from_secs(5), async {
            loop {
                if let Some(thread_id) = manager.thread_binding(&job_id) {
                    let status = manager
                        .job_runtime_summary(&thread_id)
                        .map(|runtime| runtime.status);
                    if matches!(status, Some(ThreadRuntimeStatus::Cooling)) {
                        break thread_id;
                    }
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("job runtime should cool after completion");

        manager
            .thread_pool()
            .evict_runtime(&execution_thread_id, ThreadPoolEventReason::CoolingExpired)
            .await
            .expect("cooling job runtime should be evictable");

        let mut saw_updated = false;
        let mut saw_evicted = false;
        timeout(Duration::from_secs(5), async {
            while !(saw_updated && saw_evicted) {
                match parent_rx.recv().await {
                    Ok(ThreadEvent::JobRuntimeUpdated { runtime })
                        if runtime.job_id == job_id
                            && runtime.thread_id == execution_thread_id
                            && runtime.status == ThreadRuntimeStatus::Evicted
                            && runtime.last_reason
                                == Some(ThreadPoolEventReason::CoolingExpired) =>
                    {
                        saw_updated = true;
                    }
                    Ok(ThreadEvent::JobRuntimeEvicted {
                        thread_id,
                        job_id: event_job_id,
                        reason,
                    }) if event_job_id == job_id
                        && thread_id == execution_thread_id
                        && reason == ThreadPoolEventReason::CoolingExpired =>
                    {
                        saw_evicted = true;
                    }
                    Ok(_) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                    Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                        panic!("parent thread event channel should remain open");
                    }
                }
            }
        })
        .await
        .expect("parent thread should observe job runtime eviction");
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
    async fn stop_job_allows_loading_runtime() {
        let manager = test_job_manager();
        let thread_id = ThreadId::new();
        let job_id = "job-stop-loading".to_string();

        manager.record_dispatched_job(thread_id, job_id.clone());
        manager.sync_job_runtime_metadata(thread_id, Some(job_id.clone()), None);
        manager.upsert_job_runtime_summary(
            thread_id,
            job_id.clone(),
            ThreadRuntimeStatus::Loading,
            0,
            Some(Utc::now().to_rfc3339()),
            true,
            None,
        );

        manager
            .stop_job(&job_id)
            .expect("stop_job should succeed while runtime is loading");
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
    async fn stop_job_cancels_turn_and_broadcasts_cancelled_job_result() {
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
                    .job_runtime_state()
                    .runtimes
                    .into_iter()
                    .find(|runtime| runtime.job_id == job_id)
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
                        cancelled,
                        message,
                        ..
                    }) if event_job_id == job_id => {
                        assert_eq!(thread_id, originating_thread_id);
                        break (success, cancelled, message);
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

        assert!(
            !job_event.0,
            "cancelled job should still report unsuccessful execution"
        );
        assert!(
            job_event.1,
            "cancelled job should report cancellation explicitly"
        );
        assert!(
            job_event.2.contains("Turn cancelled"),
            "unexpected cancel message: {}",
            job_event.2
        );

        assert!(matches!(
            manager.get_job_result_status(originating_thread_id, &job_id, false),
            JobLookup::Completed(ThreadJobResult {
                success: false,
                cancelled: true,
                ..
            })
        ));

        let persisted_job = manager
            .job_repository
            .as_ref()
            .expect("test manager should expose a job repository")
            .get(&argus_repository::types::JobId::new(job_id.clone()))
            .await
            .expect("cancelled job should persist")
            .expect("cancelled job record should exist");
        assert_eq!(persisted_job.status, JobStatus::Cancelled);

        let runtime = manager
            .job_runtime_state()
            .runtimes
            .into_iter()
            .find(|runtime| runtime.job_id == job_id)
            .expect("cancelled job runtime should remain tracked");
        assert_eq!(runtime.status, ThreadRuntimeStatus::Cooling);
        assert_eq!(runtime.last_reason, Some(ThreadPoolEventReason::Cancelled));
    }

    #[tokio::test]
    async fn recover_parent_then_children_keeps_persisted_job_id_authoritative() {
        let provider = Arc::new(CapturingProvider::new(
            "done",
            Duration::from_millis(1),
            8,
        ));
        let (manager, agent_id, parent_thread_id) = test_job_manager_with_provider(provider).await;
        let child_thread_id = ThreadId::new();
        let child_job_id = "job-cache-authority".to_string();
        let parent_base_dir = manager
            .trace_base_dir_for_thread(parent_thread_id)
            .await
            .expect("parent trace dir should exist");
        let parent_metadata = recover_thread_metadata(&parent_base_dir)
            .await
            .expect("parent metadata should recover");
        let child_base_dir = child_thread_base_dir(&parent_base_dir, child_thread_id);
        let child_snapshot = manager
            .template_manager
            .get(agent_id)
            .await
            .expect("template lookup should succeed")
            .expect("agent snapshot should exist");
        persist_thread_metadata(
            &child_base_dir,
            &ThreadTraceMetadata {
                thread_id: child_thread_id,
                kind: ThreadTraceKind::Job,
                root_session_id: parent_metadata.root_session_id,
                parent_thread_id: Some(parent_thread_id),
                job_id: Some(child_job_id.clone()),
                agent_snapshot: child_snapshot,
            },
        )
        .await
        .expect("child metadata should persist");

        let recovered_parent = manager
            .recover_parent_job_thread_id(&child_thread_id)
            .await
            .expect("parent recovery should succeed");
        assert_eq!(recovered_parent, Some(parent_thread_id));

        let children = manager
            .recover_child_jobs_for_thread(parent_thread_id)
            .await
            .expect("child recovery should succeed");
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].thread_id, child_thread_id);
        assert_eq!(
            children[0].job_id, child_job_id,
            "cached child listings must preserve the persisted job id"
        );
    }
}
