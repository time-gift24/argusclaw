//! JobManager for dispatching and managing background jobs.
//!
//! Each dispatched job is tracked through a ThreadPool-managed execution thread.
//! Results are sent back through the unified pipe as ThreadEvent::JobResult.

use std::collections::HashMap;
use std::fmt;
use std::path::PathBuf;
use std::sync::{Arc, Mutex as StdMutex};

use argus_agent::CompactorManager;
#[cfg(test)]
use argus_agent::TurnOutput;
#[cfg(test)]
use argus_protocol::llm::{ChatMessage, Role};
use argus_protocol::{
    AgentId, ProviderResolver, ThreadControlEvent, ThreadEvent, ThreadId, ThreadJobResult,
    ThreadPoolRuntimeKind, ThreadPoolRuntimeRef, ThreadPoolSnapshot, ThreadPoolState,
};
use argus_repository::traits::{JobRepository, LlmProviderRepository, ThreadRepository};
use argus_template::TemplateManager;
use argus_tool::ToolManager;
use tokio::sync::{broadcast, mpsc};

use crate::error::JobError;
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
    thread_pool: Arc<ThreadPool>,
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
        Self::new_with_persistence(
            template_manager,
            provider_resolver,
            tool_manager,
            compactor_manager,
            trace_dir,
            None,
        )
    }

    /// Create a new JobManager with optional persistent thread-pool backing.
    pub fn new_with_persistence(
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        tool_manager: Arc<ToolManager>,
        compactor_manager: Arc<CompactorManager>,
        trace_dir: PathBuf,
        persistence: Option<ThreadPoolPersistence>,
    ) -> Self {
        let thread_pool = Arc::new(ThreadPool::with_persistence(
            template_manager,
            provider_resolver,
            tool_manager,
            compactor_manager,
            trace_dir,
            persistence,
        ));

        Self {
            thread_pool,
            tracked_jobs: Arc::new(StdMutex::new(HashMap::new())),
        }
    }

    /// Create a new JobManager wired with repository-backed persistence.
    #[allow(clippy::too_many_arguments)]
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
    use std::sync::Arc;

    use argus_protocol::llm::LlmProviderRepository;
    use argus_protocol::{LlmProvider, ProviderId};
    use argus_repository::ArgusSqlite;
    use argus_repository::migrate;
    use argus_repository::traits::{AgentRepository, JobRepository, ThreadRepository};
    use argus_template::TemplateManager;
    use async_trait::async_trait;
    use sqlx::SqlitePool;

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
    async fn dispatch_job_creates_thread_pool_binding() {
        let manager = test_job_manager();
        let originating_thread_id = ThreadId::new();
        let (pipe_tx, _pipe_rx) = broadcast::channel(16);
        let (control_tx, _control_rx) = mpsc::unbounded_channel();
        let job_id = "job-bound".to_string();

        manager
            .dispatch_job(
                originating_thread_id,
                job_id.clone(),
                AgentId::new(99),
                "run this".to_string(),
                None,
                pipe_tx,
                control_tx,
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
        let (control_tx, mut control_rx) = mpsc::unbounded_channel();
        let job_id = "alpha-job-event-flow".to_string();

        manager
            .dispatch_job(
                originating_thread_id,
                job_id.clone(),
                AgentId::new(99),
                "run alpha event flow".to_string(),
                None,
                pipe_tx,
                control_tx,
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

        let control_event = timeout(Duration::from_secs(1), control_rx.recv())
            .await
            .expect("forwarded control event should arrive")
            .expect("control channel should stay open");
        assert!(matches!(
            control_event,
            ThreadControlEvent::JobResult(result)
                if result.job_id == job_id && !result.success
        ));

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
        let (control_tx, _control_rx) = mpsc::unbounded_channel();
        let job_id = "job-enqueue-failure".to_string();

        let dispatch_result = manager
            .dispatch_job(
                originating_thread_id,
                job_id.clone(),
                AgentId::new(999_999),
                "run this".to_string(),
                None,
                pipe_tx,
                control_tx,
            )
            .await;

        assert!(dispatch_result.is_err());
        assert!(matches!(
            manager.get_job_result_status(originating_thread_id, &job_id, false),
            JobLookup::NotFound
        ));
    }
}
