//! ThreadPool for coordinating unified job and chat runtimes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use crate::error::JobError;
use crate::types::ThreadPoolJobRequest;
use argus_agent::config::ThreadConfigBuilder;
use argus_agent::thread_trace_store::{
    ThreadTraceKind, ThreadTraceMetadata, chat_thread_base_dir, child_thread_base_dir,
    find_job_thread_base_dir, list_direct_child_threads, persist_thread_metadata,
    recover_thread_metadata,
};
use argus_agent::turn_log_store::recover_thread_log_state;
use argus_agent::{
    FilePlanStore, LlmThreadCompactor, ThreadBuilder, TraceConfig, TurnCancellation, TurnConfig,
};
use argus_protocol::llm::{
    ChatMessage, CompletionRequest, CompletionResponse, LlmError, LlmEventStream, Role,
};
use argus_protocol::{
    AgentId, LlmProvider, MailboxMessage, MailboxMessageType, McpToolResolver, ProviderId,
    ProviderResolver, SessionId, ThreadControlMessage, ThreadEvent, ThreadId, ThreadJobResult,
    ThreadMessage, ThreadPoolEventReason, ThreadPoolRuntimeKind, ThreadPoolRuntimeSummary,
    ThreadPoolSnapshot, ThreadPoolState, ThreadRuntimeStatus,
};
use argus_repository::traits::{JobRepository, LlmProviderRepository, ThreadRepository};
use argus_repository::types::{
    AgentId as RepoAgentId, JobId, JobRecord, JobResult, JobStatus, JobType, ThreadRecord,
};
use argus_template::TemplateManager;
use argus_tool::ToolManager;
use chrono::Utc;
use rust_decimal::Decimal;
use tokio::sync::{Mutex as AsyncMutex, OwnedSemaphorePermit, RwLock, Semaphore, broadcast};
use tokio::task::AbortHandle;
use uuid::Uuid;

const DEFAULT_MAX_THREADS: u32 = 8;

#[derive(Clone)]
struct ChatRuntimeConfig {
    trace_dir: PathBuf,
}

impl std::fmt::Debug for ChatRuntimeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatRuntimeConfig")
            .field("trace_dir", &self.trace_dir)
            .finish()
    }
}

#[derive(Debug)]
struct UnavailableChatProvider {
    model_name: String,
    reason: String,
}

impl UnavailableChatProvider {
    fn new(model_name: String, reason: String) -> Self {
        Self { model_name, reason }
    }

    fn llm_error(&self) -> LlmError {
        LlmError::RequestFailed {
            provider: self.model_name.clone(),
            reason: self.reason.clone(),
        }
    }
}

#[async_trait::async_trait]
impl argus_protocol::LlmProvider for UnavailableChatProvider {
    fn model_name(&self) -> &str {
        &self.model_name
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(
        &self,
        _request: CompletionRequest,
    ) -> std::result::Result<CompletionResponse, LlmError> {
        Err(self.llm_error())
    }

    async fn stream_complete(
        &self,
        _request: CompletionRequest,
    ) -> std::result::Result<LlmEventStream, LlmError> {
        Err(self.llm_error())
    }
}

#[derive(Debug)]
struct RuntimeEntry {
    summary: ThreadPoolRuntimeSummary,
    sender: broadcast::Sender<ThreadEvent>,
    thread: Option<Arc<RwLock<argus_agent::Thread>>>,
    queued_job_results: Vec<MailboxMessage>,
    forwarder_abort: Option<AbortHandle>,
    slot_permit: Option<OwnedSemaphorePermit>,
    load_mutex: Arc<AsyncMutex<()>>,
}

#[derive(Debug, Default)]
struct ThreadPoolStore {
    runtimes: HashMap<String, RuntimeEntry>,
    job_bindings: HashMap<String, ThreadId>,
    parent_thread_by_child: HashMap<ThreadId, ThreadId>,
    child_threads_by_parent: HashMap<ThreadId, Vec<ThreadId>>,
    peak_estimated_memory_bytes: u64,
    peak_process_memory_bytes: Option<u64>,
}

#[derive(Debug, Default)]
struct RuntimeShutdown {
    thread: Option<Arc<RwLock<argus_agent::Thread>>>,
    forwarder_abort: Option<AbortHandle>,
}

impl RuntimeShutdown {
    fn run(self) {
        if let Some(forwarder_abort) = self.forwarder_abort {
            forwarder_abort.abort();
        }
        if let Some(thread) = self.thread {
            tokio::spawn(async move {
                let _ = thread.read().await.send_message(ThreadMessage::Control(
                    ThreadControlMessage::ShutdownRuntime,
                ));
            });
        }
    }
}

#[derive(Clone)]
pub struct ThreadPoolPersistence {
    job_repository: Arc<dyn JobRepository>,
    thread_repository: Arc<dyn ThreadRepository>,
    provider_repository: Arc<dyn LlmProviderRepository>,
}

impl ThreadPoolPersistence {
    #[must_use]
    pub fn new(
        job_repository: Arc<dyn JobRepository>,
        thread_repository: Arc<dyn ThreadRepository>,
        provider_repository: Arc<dyn LlmProviderRepository>,
    ) -> Self {
        Self {
            job_repository,
            thread_repository,
            provider_repository,
        }
    }

    pub(crate) fn job_repository(&self) -> Arc<dyn JobRepository> {
        Arc::clone(&self.job_repository)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecoveredChildJob {
    pub thread_id: ThreadId,
    pub job_id: String,
}

/// Coordinates job-thread bindings, runtime state transitions, and metrics.
pub struct ThreadPool {
    template_manager: Arc<TemplateManager>,
    provider_resolver: Arc<dyn ProviderResolver>,
    tool_manager: Arc<ToolManager>,
    mcp_tool_resolver: Arc<StdMutex<Option<Arc<dyn McpToolResolver>>>>,
    chat_runtime_config: ChatRuntimeConfig,
    persistence: Option<ThreadPoolPersistence>,
    max_threads: u32,
    resident_slots: Arc<Semaphore>,
    admission_waiters: Arc<AtomicUsize>,
    store: Arc<StdMutex<ThreadPoolStore>>,
}

impl std::fmt::Debug for ThreadPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThreadPool")
            .field("max_threads", &self.max_threads)
            .finish()
    }
}

impl ThreadPool {
    /// Create a new thread pool with a default runtime cap.
    pub fn new(
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        tool_manager: Arc<ToolManager>,
        trace_dir: PathBuf,
    ) -> Self {
        Self::with_persistence(
            template_manager,
            provider_resolver,
            tool_manager,
            trace_dir,
            None,
        )
    }

    /// Create a thread pool with optional repository-backed persistence.
    pub fn with_persistence(
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        tool_manager: Arc<ToolManager>,
        trace_dir: PathBuf,
        persistence: Option<ThreadPoolPersistence>,
    ) -> Self {
        Self {
            template_manager,
            provider_resolver,
            tool_manager,
            mcp_tool_resolver: Arc::new(StdMutex::new(None)),
            chat_runtime_config: ChatRuntimeConfig { trace_dir },
            persistence,
            max_threads: DEFAULT_MAX_THREADS,
            resident_slots: Arc::new(Semaphore::new(DEFAULT_MAX_THREADS as usize)),
            admission_waiters: Arc::new(AtomicUsize::new(0)),
            store: Arc::new(StdMutex::new(ThreadPoolStore::default())),
        }
    }

    pub fn set_mcp_tool_resolver(&self, resolver: Option<Arc<dyn McpToolResolver>>) {
        *self
            .mcp_tool_resolver
            .lock()
            .expect("mcp resolver mutex poisoned") = resolver;
    }

    /// Register a chat thread in the unified pool without loading its runtime.
    pub fn register_chat_thread(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> broadcast::Receiver<ThreadEvent> {
        self.upsert_runtime_summary(
            thread_id,
            ThreadPoolRuntimeKind::Chat,
            Some(session_id),
            None,
            ThreadRuntimeStatus::Inactive,
            0,
            None,
            true,
            None,
            None,
        )
        .subscribe()
    }

    /// Subscribe to a registered runtime.
    pub fn subscribe(&self, thread_id: &ThreadId) -> Option<broadcast::Receiver<ThreadEvent>> {
        self.store
            .lock()
            .expect("thread-pool mutex poisoned")
            .runtimes
            .get(&thread_id.to_string())
            .map(|entry| entry.sender.subscribe())
    }

    /// Remove a runtime from the pool registry.
    pub fn remove_runtime(&self, thread_id: &ThreadId) -> bool {
        let mut store = self.store.lock().expect("thread-pool mutex poisoned");
        let removed_entry = store.runtimes.remove(&thread_id.to_string());
        let removed = removed_entry.is_some();
        if removed {
            store
                .job_bindings
                .retain(|_, bound_thread_id| bound_thread_id != thread_id);
            if let Some(parent_thread_id) = store.parent_thread_by_child.remove(thread_id)
                && let Some(children) = store.child_threads_by_parent.get_mut(&parent_thread_id)
            {
                children.retain(|child_thread_id| child_thread_id != thread_id);
                if children.is_empty() {
                    store.child_threads_by_parent.remove(&parent_thread_id);
                }
            }
            Self::refresh_peaks(&mut store);
        }
        drop(store);

        if let Some(entry) = removed_entry {
            RuntimeShutdown {
                thread: entry.thread,
                forwarder_abort: entry.forwarder_abort,
            }
            .run();
        }

        removed
    }

    /// Return a currently loaded chat runtime, if present.
    pub fn loaded_chat_thread(
        &self,
        thread_id: &ThreadId,
    ) -> Option<Arc<RwLock<argus_agent::Thread>>> {
        self.store
            .lock()
            .expect("thread-pool mutex poisoned")
            .runtimes
            .get(&thread_id.to_string())
            .and_then(|entry| {
                (entry.summary.kind == ThreadPoolRuntimeKind::Chat)
                    .then(|| entry.thread.clone())
                    .flatten()
            })
    }

    /// Return a currently loaded runtime thread, if present.
    pub fn loaded_thread(&self, thread_id: &ThreadId) -> Option<Arc<RwLock<argus_agent::Thread>>> {
        self.store
            .lock()
            .expect("thread-pool mutex poisoned")
            .runtimes
            .get(&thread_id.to_string())
            .and_then(|entry| entry.thread.clone())
    }

    /// Return the direct parent thread for a child job runtime.
    pub fn parent_thread_id(&self, child_thread_id: &ThreadId) -> Option<ThreadId> {
        self.store
            .lock()
            .expect("thread-pool mutex poisoned")
            .parent_thread_by_child
            .get(child_thread_id)
            .copied()
    }

    /// Return direct child job runtimes for a thread.
    pub fn child_thread_ids(&self, parent_thread_id: &ThreadId) -> Vec<ThreadId> {
        self.store
            .lock()
            .expect("thread-pool mutex poisoned")
            .child_threads_by_parent
            .get(parent_thread_id)
            .cloned()
            .unwrap_or_default()
    }

    /// Recover the direct parent thread for a persisted child job runtime.
    pub async fn recover_parent_thread_id(
        &self,
        child_thread_id: &ThreadId,
    ) -> Result<Option<ThreadId>, JobError> {
        if let Some(parent_thread_id) = self.parent_thread_id(child_thread_id) {
            return Ok(Some(parent_thread_id));
        }

        Ok(self
            .recover_job_thread_metadata(*child_thread_id)
            .await?
            .and_then(|metadata| metadata.parent_thread_id))
    }

    /// Recover the bound execution thread for a job from persistence when caches are cold.
    pub async fn recover_thread_binding(&self, job_id: &str) -> Result<Option<ThreadId>, JobError> {
        if let Some(thread_id) = self.get_thread_binding(job_id) {
            return Ok(Some(thread_id));
        }

        let Some(persistence) = &self.persistence else {
            return Ok(None);
        };
        let Some(job_record) = persistence
            .job_repository
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
        let Some(metadata) = self.recover_job_thread_metadata(thread_id).await? else {
            return Ok(None);
        };
        if metadata.job_id.as_deref() != Some(job_id) {
            return Err(JobError::ExecutionFailed(format!(
                "job {job_id} is bound to thread {thread_id}, but trace metadata recorded {:?}",
                metadata.job_id
            )));
        }

        self.store
            .lock()
            .expect("thread-pool mutex poisoned")
            .job_bindings
            .insert(job_id.to_string(), thread_id);
        Ok(Some(thread_id))
    }

    /// Recover direct persisted child jobs for a parent thread.
    pub async fn recover_child_jobs(
        &self,
        parent_thread_id: ThreadId,
    ) -> Result<Vec<RecoveredChildJob>, JobError> {
        let parent_base_dir = self.trace_base_dir_for_thread(parent_thread_id).await?;
        let metadata = list_direct_child_threads(&parent_base_dir, parent_thread_id)
            .await
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        let mut recovered = Vec::with_capacity(metadata.len());

        for child_metadata in metadata {
            let job_id = child_metadata.job_id.clone().ok_or_else(|| {
                JobError::ExecutionFailed(format!(
                    "job thread {} is missing persisted job_id metadata",
                    child_metadata.thread_id
                ))
            })?;
            self.sync_relationship_cache(&child_metadata);
            recovered.push(RecoveredChildJob {
                thread_id: child_metadata.thread_id,
                job_id,
            });
        }

        Ok(recovered)
    }

    /// Return the current runtime summary for a thread.
    pub fn runtime_summary(&self, thread_id: &ThreadId) -> Option<ThreadPoolRuntimeSummary> {
        self.store
            .lock()
            .expect("thread-pool mutex poisoned")
            .runtimes
            .get(&thread_id.to_string())
            .map(|entry| entry.summary.clone())
    }

    fn route_mailbox_message(message: MailboxMessage) -> ThreadMessage {
        if matches!(message.message_type, MailboxMessageType::JobResult { .. }) {
            ThreadMessage::JobResult { message }
        } else {
            ThreadMessage::PeerMessage { message }
        }
    }

    /// Remove a queued job-result mailbox item after the persisted result is consumed.
    pub fn claim_queued_job_result(
        &self,
        thread_id: ThreadId,
        job_id: &str,
    ) -> Option<MailboxMessage> {
        let mut store = self.store.lock().expect("thread-pool mutex poisoned");
        let entry = store.runtimes.get_mut(&thread_id.to_string())?;
        let index = entry
            .queued_job_results
            .iter()
            .position(|message| message.job_id() == Some(job_id))?;
        Some(entry.queued_job_results.remove(index))
    }

    /// Deliver a mailbox message to a runtime thread.
    pub async fn deliver_mailbox_message(
        &self,
        thread_id: ThreadId,
        message: MailboxMessage,
    ) -> Result<(), JobError> {
        let (thread, session_id) = match self.runtime_summary(&thread_id) {
            Some(summary) if summary.kind == ThreadPoolRuntimeKind::Chat => {
                let session_id = summary.session_id.ok_or_else(|| {
                    JobError::ExecutionFailed(format!(
                        "chat thread {} is missing a session binding",
                        thread_id
                    ))
                })?;
                (
                    self.ensure_chat_runtime(session_id, thread_id).await?,
                    Some(session_id),
                )
            }
            Some(_) => (
                self.loaded_thread(&thread_id).ok_or_else(|| {
                    JobError::ExecutionFailed(format!("thread {} is not loaded", thread_id))
                })?,
                None,
            ),
            None => {
                return Err(JobError::ExecutionFailed(format!(
                    "thread {} is not registered",
                    thread_id
                )));
            }
        };

        if let Some(session_id) = session_id {
            let estimated_memory_bytes =
                Self::estimate_thread_memory(&thread).await + message.text.len() as u64;
            let started_at = Utc::now().to_rfc3339();
            let sender = self
                .mark_runtime_running(&thread_id, estimated_memory_bytes, started_at)
                .ok_or_else(|| {
                    JobError::ExecutionFailed(format!("thread {} is not registered", thread_id))
                })?;
            let _ = sender.send(ThreadEvent::ThreadPoolStarted {
                thread_id,
                kind: ThreadPoolRuntimeKind::Chat,
                session_id: Some(session_id),
                job_id: None,
            });
            let _ = sender.send(ThreadEvent::ThreadPoolMetricsUpdated {
                snapshot: self.collect_metrics(),
            });
        }

        let routed_message = Self::route_mailbox_message(message.clone());
        thread
            .read()
            .await
            .send_message(routed_message)
            .map_err(|error| JobError::ExecutionFailed(error.to_string()))?;
        if matches!(message.message_type, MailboxMessageType::JobResult { .. })
            && let Some(entry) = self
                .store
                .lock()
                .expect("thread-pool mutex poisoned")
                .runtimes
                .get_mut(&thread_id.to_string())
        {
            entry.queued_job_results.push(message.clone());
        }

        if let Some(sender) = self
            .store
            .lock()
            .expect("thread-pool mutex poisoned")
            .runtimes
            .get(&thread_id.to_string())
            .map(|entry| entry.sender.clone())
        {
            let _ = sender.send(ThreadEvent::MailboxMessageQueued { thread_id, message });
        }

        Ok(())
    }

    /// Return the authoritative pool state used by external observers.
    pub fn collect_state(&self) -> ThreadPoolState {
        let store = self.store.lock().expect("thread-pool mutex poisoned");
        ThreadPoolState {
            snapshot: Self::collect_metrics_from_store(self.max_threads, &store),
            runtimes: store
                .runtimes
                .values()
                .map(|entry| entry.summary.clone())
                .collect(),
        }
    }

    /// Bind a job to a concrete execution thread and mark it queued.
    pub async fn enqueue_job(&self, request: ThreadPoolJobRequest) -> Result<ThreadId, JobError> {
        let now = Utc::now().to_rfc3339();
        let thread_id = self.persist_binding(&request, &now).await?;
        self.persist_job_status(&request.job_id, JobStatus::Queued, None, None)
            .await?;
        self.upsert_runtime_summary(
            thread_id,
            ThreadPoolRuntimeKind::Job,
            None,
            Some(request.job_id.clone()),
            ThreadRuntimeStatus::Queued,
            request.prompt.len() as u64,
            Some(now),
            true,
            None,
            None,
        );
        self.store
            .lock()
            .expect("thread-pool mutex poisoned")
            .job_bindings
            .insert(request.job_id.clone(), thread_id);
        {
            let mut store = self.store.lock().expect("thread-pool mutex poisoned");
            store
                .parent_thread_by_child
                .insert(thread_id, request.originating_thread_id);
            let children = store
                .child_threads_by_parent
                .entry(request.originating_thread_id)
                .or_default();
            if !children.contains(&thread_id) {
                children.push(thread_id);
            }
        }
        Ok(thread_id)
    }

    /// Return the currently bound thread for a job.
    pub fn get_thread_binding(&self, job_id: &str) -> Option<ThreadId> {
        self.store
            .lock()
            .expect("thread-pool mutex poisoned")
            .job_bindings
            .get(job_id)
            .copied()
    }

    /// Evict a chat runtime that is currently cooling.
    pub fn evict_chat_if_idle(&self, thread_id: &ThreadId) -> Option<ThreadPoolRuntimeSummary> {
        self.evict_runtime(thread_id, ThreadPoolEventReason::CoolingExpired)
    }

    /// Collect a point-in-time metrics snapshot for the pool.
    pub fn collect_metrics(&self) -> ThreadPoolSnapshot {
        let store = self.store.lock().expect("thread-pool mutex poisoned");
        Self::collect_metrics_from_store(self.max_threads, &store)
    }

    /// Queue a user message onto a chat runtime, loading it on demand.
    pub async fn send_chat_message(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
        message: String,
    ) -> Result<(), JobError> {
        self.register_chat_thread(session_id, thread_id);
        let thread = self.ensure_chat_runtime(session_id, thread_id).await?;
        let estimated_memory_bytes =
            Self::estimate_thread_memory(&thread).await + message.len() as u64;
        let started_at = Utc::now().to_rfc3339();
        let sender = self
            .mark_runtime_running(&thread_id, estimated_memory_bytes, started_at)
            .ok_or_else(|| {
                JobError::ExecutionFailed(format!("thread {} is not registered", thread_id))
            })?;

        let _ = sender.send(ThreadEvent::ThreadPoolStarted {
            thread_id,
            kind: ThreadPoolRuntimeKind::Chat,
            session_id: Some(session_id),
            job_id: None,
        });
        let _ = sender.send(ThreadEvent::ThreadPoolMetricsUpdated {
            snapshot: self.collect_metrics(),
        });

        thread
            .read()
            .await
            .send_message(ThreadMessage::UserInput {
                content: message,
                msg_override: None,
            })
            .map_err(|error| JobError::ExecutionFailed(error.to_string()))?;

        Ok(())
    }

    /// Ensure a chat runtime is resident and ready for message delivery.
    pub async fn ensure_chat_runtime(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Result<Arc<RwLock<argus_agent::Thread>>, JobError> {
        if let Some(thread) = self.loaded_runtime(&thread_id) {
            return Ok(thread);
        }

        self.upsert_runtime_summary(
            thread_id,
            ThreadPoolRuntimeKind::Chat,
            Some(session_id),
            None,
            ThreadRuntimeStatus::Loading,
            0,
            Some(Utc::now().to_rfc3339()),
            true,
            None,
            None,
        );
        let load_mutex = self.runtime_load_mutex(&thread_id)?;
        let _load_guard = load_mutex.lock().await;
        if let Some(thread) = self.loaded_runtime(&thread_id) {
            return Ok(thread);
        }

        self.ensure_runtime_slot(&thread_id).await?;

        let thread = match self.build_chat_thread(session_id, thread_id).await {
            Ok(thread) => thread,
            Err(error) => {
                self.reset_runtime_after_load_failure(
                    &thread_id,
                    ThreadPoolEventReason::ExecutionFailed,
                );
                return Err(error);
            }
        };
        let runtime_rx = {
            let guard = thread.read().await;
            guard.subscribe()
        };
        argus_agent::Thread::spawn_reactor(Arc::clone(&thread)).await;
        self.attach_chat_runtime(thread_id, session_id, Arc::clone(&thread), runtime_rx)
            .await?;
        Ok(thread)
    }

    /// Execute an enqueued job on its bound thread runtime.
    #[allow(clippy::too_many_arguments)]
    pub async fn execute_job(
        &self,
        request: ThreadPoolJobRequest,
        execution_thread_id: ThreadId,
        pipe_tx: broadcast::Sender<ThreadEvent>,
        cancellation: TurnCancellation,
    ) -> ThreadJobResult {
        let fallback_job_id = request.job_id.clone();
        let fallback_agent_id = request.agent_id;
        let fallback_display_name = format!("Agent {}", fallback_agent_id.inner());
        let thread = match self.ensure_job_runtime(&request, execution_thread_id).await {
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
        let runtime_rx = match self.subscribe(&execution_thread_id) {
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
            Self::estimate_thread_memory(&thread).await + request.prompt.len() as u64;
        self.mark_runtime_running(
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
        let _ = pipe_tx.send(ThreadEvent::ThreadPoolStarted {
            thread_id: execution_thread_id,
            kind: ThreadPoolRuntimeKind::Job,
            session_id: None,
            job_id: Some(request.job_id.clone()),
        });
        let _ = pipe_tx.send(ThreadEvent::ThreadPoolMetricsUpdated {
            snapshot: self.collect_metrics(),
        });

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
                Self::failure_result(
                    fallback_job_id,
                    fallback_agent_id,
                    fallback_display_name,
                    String::new(),
                    "Turn cancelled".to_string(),
                )
            } else {
                match self
                    .deliver_mailbox_message(execution_thread_id, task_assignment)
                    .await
                {
                    Ok(()) => {
                        let cancellation_thread = Arc::clone(&thread);
                        let cancellation_signal = cancellation.clone();
                        let cancellation_forwarder = tokio::spawn(async move {
                            cancellation_signal.cancelled().await;
                            let _ = cancellation_thread
                                .read()
                                .await
                                .send_message(ThreadMessage::Interrupt);
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

        let cooling_memory = Self::estimate_thread_memory(&thread).await;

        if let Some((runtime, _sender, snapshot)) =
            self.transition_runtime_to_cooling(&execution_thread_id, Some(cooling_memory))
        {
            #[allow(clippy::collapsible_if)]
            if let Some(shutdown) = Self::emit_cooling_or_evict(
                &self.store,
                self.max_threads,
                &self.admission_waiters,
                &execution_thread_id,
                &pipe_tx,
                runtime,
                snapshot,
            ) {
                shutdown.run();
            }
        }

        result
    }

    fn mark_runtime_running(
        &self,
        thread_id: &ThreadId,
        estimated_memory_bytes: u64,
        started_at: String,
    ) -> Option<broadcast::Sender<ThreadEvent>> {
        let mut store = self.store.lock().expect("thread-pool mutex poisoned");
        let entry = store.runtimes.get_mut(&thread_id.to_string())?;
        entry.summary.status = ThreadRuntimeStatus::Running;
        entry.summary.estimated_memory_bytes = estimated_memory_bytes;
        entry.summary.last_active_at = Some(started_at);
        entry.summary.last_reason = None;
        let sender = entry.sender.clone();
        Self::refresh_peaks(&mut store);
        Some(sender)
    }

    fn total_estimated_memory(store: &ThreadPoolStore) -> u64 {
        store
            .runtimes
            .values()
            .filter(|entry| entry.slot_permit.is_some())
            .map(|entry| entry.summary.estimated_memory_bytes)
            .sum()
    }

    fn refresh_peaks(store: &mut ThreadPoolStore) {
        let estimated = Self::total_estimated_memory(store);
        if estimated > store.peak_estimated_memory_bytes {
            store.peak_estimated_memory_bytes = estimated;
        }
    }

    fn collect_metrics_from_store(max_threads: u32, store: &ThreadPoolStore) -> ThreadPoolSnapshot {
        let queued_threads = store
            .runtimes
            .values()
            .filter(|entry| entry.summary.status == ThreadRuntimeStatus::Queued)
            .count() as u32;
        let running_threads = store
            .runtimes
            .values()
            .filter(|entry| entry.summary.status == ThreadRuntimeStatus::Running)
            .count() as u32;
        let cooling_threads = store
            .runtimes
            .values()
            .filter(|entry| entry.summary.status == ThreadRuntimeStatus::Cooling)
            .count() as u32;
        let resident_thread_count = store
            .runtimes
            .values()
            .filter(|entry| entry.slot_permit.is_some())
            .count() as u32;
        let estimated_memory_bytes = Self::total_estimated_memory(store);
        let avg_thread_memory_bytes = if resident_thread_count == 0 {
            0
        } else {
            estimated_memory_bytes / u64::from(resident_thread_count)
        };

        ThreadPoolSnapshot {
            max_threads,
            active_threads: resident_thread_count,
            queued_threads,
            running_threads,
            cooling_threads,
            evicted_threads: store
                .runtimes
                .values()
                .filter(|entry| entry.summary.status == ThreadRuntimeStatus::Evicted)
                .count() as u64,
            estimated_memory_bytes,
            peak_estimated_memory_bytes: store.peak_estimated_memory_bytes,
            process_memory_bytes: None,
            peak_process_memory_bytes: store.peak_process_memory_bytes,
            resident_thread_count,
            avg_thread_memory_bytes,
            captured_at: Utc::now().to_rfc3339(),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn upsert_runtime_summary(
        &self,
        thread_id: ThreadId,
        kind: ThreadPoolRuntimeKind,
        session_id: Option<SessionId>,
        job_id: Option<String>,
        status: ThreadRuntimeStatus,
        estimated_memory_bytes: u64,
        last_active_at: Option<String>,
        recoverable: bool,
        last_reason: Option<ThreadPoolEventReason>,
        thread: Option<Arc<RwLock<argus_agent::Thread>>>,
    ) -> broadcast::Sender<ThreadEvent> {
        let mut store = self.store.lock().expect("thread-pool mutex poisoned");
        let runtime_key = thread_id.to_string();
        let (
            sender,
            existing_thread,
            existing_queued_job_results,
            existing_forwarder_abort,
            existing_slot_permit,
            load_mutex,
        ) = if let Some(entry) = store.runtimes.get_mut(&runtime_key) {
            (
                entry.sender.clone(),
                entry.thread.clone(),
                entry.queued_job_results.clone(),
                entry.forwarder_abort.take(),
                entry.slot_permit.take(),
                Arc::clone(&entry.load_mutex),
            )
        } else {
            let (sender, _rx) = broadcast::channel(256);
            (
                sender,
                None,
                Vec::new(),
                None,
                None,
                Arc::new(AsyncMutex::new(())),
            )
        };
        store.runtimes.insert(
            runtime_key,
            RuntimeEntry {
                summary: ThreadPoolRuntimeSummary {
                    thread_id,
                    kind,
                    session_id,
                    job_id,
                    status,
                    estimated_memory_bytes,
                    last_active_at,
                    recoverable,
                    last_reason,
                },
                sender: sender.clone(),
                thread: thread.or(existing_thread),
                queued_job_results: existing_queued_job_results,
                forwarder_abort: existing_forwarder_abort,
                slot_permit: existing_slot_permit,
                load_mutex,
            },
        );
        Self::refresh_peaks(&mut store);
        sender
    }

    fn loaded_runtime(&self, thread_id: &ThreadId) -> Option<Arc<RwLock<argus_agent::Thread>>> {
        self.store
            .lock()
            .expect("thread-pool mutex poisoned")
            .runtimes
            .get(&thread_id.to_string())
            .and_then(|entry| entry.thread.clone())
    }

    fn runtime_load_mutex(&self, thread_id: &ThreadId) -> Result<Arc<AsyncMutex<()>>, JobError> {
        self.store
            .lock()
            .expect("thread-pool mutex poisoned")
            .runtimes
            .get(&thread_id.to_string())
            .map(|entry| Arc::clone(&entry.load_mutex))
            .ok_or_else(|| {
                JobError::ExecutionFailed(format!("thread {} is not registered", thread_id))
            })
    }

    fn take_runtime_shutdown(entry: &mut RuntimeEntry) -> RuntimeShutdown {
        RuntimeShutdown {
            thread: entry.thread.take(),
            forwarder_abort: entry.forwarder_abort.take(),
        }
    }

    async fn ensure_runtime_slot(&self, thread_id: &ThreadId) -> Result<(), JobError> {
        {
            let store = self.store.lock().expect("thread-pool mutex poisoned");
            if store
                .runtimes
                .get(&thread_id.to_string())
                .and_then(|entry| entry.slot_permit.as_ref())
                .is_some()
            {
                return Ok(());
            }
        }

        let permit = self.acquire_runtime_slot().await?;
        let mut store = self.store.lock().expect("thread-pool mutex poisoned");
        let entry = store
            .runtimes
            .get_mut(&thread_id.to_string())
            .ok_or_else(|| {
                JobError::ExecutionFailed(format!("thread {} is not registered", thread_id))
            })?;
        if entry.slot_permit.is_none() {
            entry.slot_permit = Some(permit);
            Self::refresh_peaks(&mut store);
        }
        Ok(())
    }

    async fn acquire_runtime_slot(&self) -> Result<OwnedSemaphorePermit, JobError> {
        loop {
            match Arc::clone(&self.resident_slots).try_acquire_owned() {
                Ok(permit) => return Ok(permit),
                Err(tokio::sync::TryAcquireError::Closed) => {
                    return Err(JobError::ExecutionFailed(
                        "thread pool capacity manager closed".to_string(),
                    ));
                }
                Err(tokio::sync::TryAcquireError::NoPermits) => {}
            }

            if self
                .evict_oldest_cooling_runtime(ThreadPoolEventReason::MemoryPressure)
                .is_some()
            {
                continue;
            }

            self.admission_waiters.fetch_add(1, Ordering::SeqCst);
            let permit = Arc::clone(&self.resident_slots)
                .acquire_owned()
                .await
                .map_err(|_| {
                    JobError::ExecutionFailed("thread pool capacity manager closed".to_string())
                });
            self.admission_waiters.fetch_sub(1, Ordering::SeqCst);
            return permit;
        }
    }

    fn transition_runtime_to_cooling(
        &self,
        thread_id: &ThreadId,
        estimated_memory_bytes: Option<u64>,
    ) -> Option<(
        ThreadPoolRuntimeSummary,
        broadcast::Sender<ThreadEvent>,
        ThreadPoolSnapshot,
    )> {
        let mut store = self.store.lock().expect("thread-pool mutex poisoned");
        let entry = store.runtimes.get_mut(&thread_id.to_string())?;
        entry.summary.status = ThreadRuntimeStatus::Cooling;
        if let Some(estimated_memory_bytes) = estimated_memory_bytes {
            entry.summary.estimated_memory_bytes = estimated_memory_bytes;
        }
        entry.summary.last_active_at = Some(Utc::now().to_rfc3339());
        entry.summary.last_reason = None;
        let runtime = entry.summary.clone();
        let sender = entry.sender.clone();
        Self::refresh_peaks(&mut store);
        let snapshot = Self::collect_metrics_from_store(self.max_threads, &store);
        Some((runtime, sender, snapshot))
    }

    fn reset_runtime_after_load_failure(
        &self,
        thread_id: &ThreadId,
        reason: ThreadPoolEventReason,
    ) {
        let mut store = self.store.lock().expect("thread-pool mutex poisoned");
        let mut shutdown = RuntimeShutdown::default();
        if let Some(entry) = store.runtimes.get_mut(&thread_id.to_string()) {
            entry.summary.status = ThreadRuntimeStatus::Inactive;
            entry.summary.estimated_memory_bytes = 0;
            entry.summary.last_active_at = Some(Utc::now().to_rfc3339());
            entry.summary.last_reason = Some(reason);
            shutdown = Self::take_runtime_shutdown(entry);
            entry.slot_permit = None;
            Self::refresh_peaks(&mut store);
        }
        drop(store);
        shutdown.run();
    }

    fn evict_oldest_cooling_runtime(
        &self,
        reason: ThreadPoolEventReason,
    ) -> Option<ThreadPoolRuntimeSummary> {
        let candidate = {
            let store = self.store.lock().expect("thread-pool mutex poisoned");
            store
                .runtimes
                .values()
                .filter(|entry| entry.summary.status == ThreadRuntimeStatus::Cooling)
                .min_by_key(|entry| entry.summary.last_active_at.clone())
                .map(|entry| entry.summary.thread_id)
        }?;
        self.evict_runtime(&candidate, reason)
    }

    async fn attach_chat_runtime(
        &self,
        thread_id: ThreadId,
        _session_id: SessionId,
        thread: Arc<RwLock<argus_agent::Thread>>,
        mut runtime_rx: broadcast::Receiver<ThreadEvent>,
    ) -> Result<(), JobError> {
        self.attach_runtime(thread_id, thread, &mut runtime_rx, "chat thread", true)
            .await
    }

    async fn attach_runtime(
        &self,
        thread_id: ThreadId,
        thread: Arc<RwLock<argus_agent::Thread>>,
        runtime_rx: &mut broadcast::Receiver<ThreadEvent>,
        runtime_label: &'static str,
        cool_on_idle: bool,
    ) -> Result<(), JobError> {
        let estimated_memory_bytes = Self::estimate_thread_memory(&thread).await;
        let (sender, replaced_runtime) = {
            let mut store = self.store.lock().expect("thread-pool mutex poisoned");
            let (sender, replaced_runtime) = {
                let Some(entry) = store.runtimes.get_mut(&thread_id.to_string()) else {
                    return Err(JobError::ExecutionFailed(format!(
                        "thread {} was removed while loading",
                        thread_id
                    )));
                };
                let replaced_runtime = if entry
                    .thread
                    .as_ref()
                    .is_some_and(|existing| !Arc::ptr_eq(existing, &thread))
                {
                    Self::take_runtime_shutdown(entry)
                } else {
                    RuntimeShutdown::default()
                };
                entry.summary.status = ThreadRuntimeStatus::Inactive;
                entry.summary.estimated_memory_bytes = estimated_memory_bytes;
                entry.summary.last_active_at = Some(Utc::now().to_rfc3339());
                entry.summary.last_reason = None;
                entry.thread = Some(Arc::clone(&thread));
                entry.forwarder_abort = None;
                (entry.sender.clone(), replaced_runtime)
            };
            Self::refresh_peaks(&mut store);
            (sender, replaced_runtime)
        };
        replaced_runtime.run();
        let store = Arc::clone(&self.store);
        let max_threads = self.max_threads;
        let persistence = self.persistence.clone();
        let thread_for_metrics = Arc::downgrade(&thread);
        let admission_waiters = Arc::clone(&self.admission_waiters);

        let mut runtime_rx = runtime_rx.resubscribe();
        let forwarder = tokio::spawn(async move {
            loop {
                match runtime_rx.recv().await {
                    Ok(event) => {
                        let _ = sender.send(event.clone());
                        if cool_on_idle && matches!(event, ThreadEvent::Idle { .. }) {
                            let Some(thread_for_metrics) = thread_for_metrics.upgrade() else {
                                break;
                            };
                            if !ThreadPool::await_runtime_idle_settle(&thread_for_metrics).await {
                                continue;
                            }
                            let estimated_memory_bytes =
                                ThreadPool::estimate_thread_memory(&thread_for_metrics).await;
                            ThreadPool::persist_thread_stats_with_persistence(
                                persistence.as_ref(),
                                &thread_id,
                                &thread_for_metrics,
                                runtime_label,
                            )
                            .await;

                            let (runtime, snapshot) = {
                                let mut store = store.lock().expect("thread-pool mutex poisoned");
                                let Some(entry) = store.runtimes.get_mut(&thread_id.to_string())
                                else {
                                    break;
                                };
                                entry.summary.status = ThreadRuntimeStatus::Cooling;
                                entry.summary.estimated_memory_bytes = estimated_memory_bytes;
                                entry.summary.last_active_at = Some(Utc::now().to_rfc3339());
                                entry.summary.last_reason = None;
                                let runtime = entry.summary.clone();
                                ThreadPool::refresh_peaks(&mut store);
                                let snapshot =
                                    ThreadPool::collect_metrics_from_store(max_threads, &store);
                                (runtime, snapshot)
                            };

                            if let Some(shutdown) = ThreadPool::emit_cooling_or_evict(
                                &store,
                                max_threads,
                                &admission_waiters,
                                &thread_id,
                                &sender,
                                runtime,
                                snapshot,
                            ) {
                                shutdown.run();
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                }
            }
        });
        let forwarder_abort = forwarder.abort_handle();
        let mut store = self.store.lock().expect("thread-pool mutex poisoned");
        let Some(entry) = store.runtimes.get_mut(&thread_id.to_string()) else {
            forwarder_abort.abort();
            return Err(JobError::ExecutionFailed(format!(
                "thread {} was removed while attaching",
                thread_id
            )));
        };
        entry.forwarder_abort = Some(forwarder_abort);
        Ok(())
    }

    async fn ensure_job_runtime(
        &self,
        request: &ThreadPoolJobRequest,
        thread_id: ThreadId,
    ) -> Result<Arc<RwLock<argus_agent::Thread>>, JobError> {
        if let Some(thread) = self.loaded_runtime(&thread_id) {
            return Ok(thread);
        }

        let load_mutex = self.runtime_load_mutex(&thread_id)?;
        let _load_guard = load_mutex.lock().await;
        if let Some(thread) = self.loaded_runtime(&thread_id) {
            return Ok(thread);
        }

        self.ensure_runtime_slot(&thread_id).await?;
        {
            let mut store = self.store.lock().expect("thread-pool mutex poisoned");
            let Some(entry) = store.runtimes.get_mut(&thread_id.to_string()) else {
                return Err(JobError::ExecutionFailed(format!(
                    "thread {} is not registered",
                    thread_id
                )));
            };
            entry.summary.status = ThreadRuntimeStatus::Loading;
            entry.summary.last_active_at = Some(Utc::now().to_rfc3339());
            entry.summary.last_reason = None;
        }

        let thread = match self.build_job_thread(request, thread_id).await {
            Ok(thread) => thread,
            Err(error) => {
                self.reset_runtime_after_load_failure(
                    &thread_id,
                    ThreadPoolEventReason::ExecutionFailed,
                );
                return Err(error);
            }
        };
        let runtime_rx = {
            let guard = thread.read().await;
            guard.subscribe()
        };
        argus_agent::Thread::spawn_reactor(Arc::clone(&thread)).await;
        if let Err(error) = self
            .attach_job_runtime(thread_id, Arc::clone(&thread), runtime_rx)
            .await
        {
            self.reset_runtime_after_load_failure(
                &thread_id,
                ThreadPoolEventReason::ExecutionFailed,
            );
            return Err(error);
        }
        Ok(thread)
    }

    async fn attach_job_runtime(
        &self,
        thread_id: ThreadId,
        thread: Arc<RwLock<argus_agent::Thread>>,
        mut runtime_rx: broadcast::Receiver<ThreadEvent>,
    ) -> Result<(), JobError> {
        self.attach_runtime(thread_id, thread, &mut runtime_rx, "job thread", false)
            .await
    }

    fn evict_runtime(
        &self,
        thread_id: &ThreadId,
        reason: ThreadPoolEventReason,
    ) -> Option<ThreadPoolRuntimeSummary> {
        let (runtime, snapshot, shutdown) = Self::evict_runtime_from_shared_store(
            &self.store,
            self.max_threads,
            thread_id,
            reason.clone(),
        )?;
        shutdown.run();
        let sender = self
            .store
            .lock()
            .expect("thread-pool mutex poisoned")
            .runtimes
            .get(&thread_id.to_string())
            .map(|entry| entry.sender.clone())?;
        let _ = sender.send(ThreadEvent::ThreadPoolEvicted {
            thread_id: runtime.thread_id,
            kind: runtime.kind,
            session_id: runtime.session_id,
            job_id: runtime.job_id.clone(),
            reason,
        });
        let _ = sender.send(ThreadEvent::ThreadPoolMetricsUpdated { snapshot });
        Some(runtime)
    }

    fn emit_cooling_or_evict(
        store: &Arc<StdMutex<ThreadPoolStore>>,
        max_threads: u32,
        admission_waiters: &AtomicUsize,
        thread_id: &ThreadId,
        sender: &broadcast::Sender<ThreadEvent>,
        runtime: ThreadPoolRuntimeSummary,
        snapshot: ThreadPoolSnapshot,
    ) -> Option<RuntimeShutdown> {
        if admission_waiters.load(Ordering::SeqCst) > 0
            && let Some((runtime, snapshot, shutdown)) = Self::evict_runtime_from_shared_store(
                store,
                max_threads,
                thread_id,
                ThreadPoolEventReason::MemoryPressure,
            )
        {
            let _ = sender.send(ThreadEvent::ThreadPoolEvicted {
                thread_id: runtime.thread_id,
                kind: runtime.kind,
                session_id: runtime.session_id,
                job_id: runtime.job_id.clone(),
                reason: ThreadPoolEventReason::MemoryPressure,
            });
            let _ = sender.send(ThreadEvent::ThreadPoolMetricsUpdated { snapshot });
            return Some(shutdown);
        }
        let _ = sender.send(ThreadEvent::ThreadPoolCooling {
            thread_id: runtime.thread_id,
            kind: runtime.kind,
            session_id: runtime.session_id,
            job_id: runtime.job_id.clone(),
        });
        let _ = sender.send(ThreadEvent::ThreadPoolMetricsUpdated { snapshot });
        None
    }

    fn evict_runtime_from_shared_store(
        store: &Arc<StdMutex<ThreadPoolStore>>,
        max_threads: u32,
        thread_id: &ThreadId,
        reason: ThreadPoolEventReason,
    ) -> Option<(
        ThreadPoolRuntimeSummary,
        ThreadPoolSnapshot,
        RuntimeShutdown,
    )> {
        let mut store = store.lock().expect("thread-pool mutex poisoned");
        let entry = store.runtimes.get_mut(&thread_id.to_string())?;
        if entry.summary.status != ThreadRuntimeStatus::Cooling {
            return None;
        }
        let shutdown = ThreadPool::take_runtime_shutdown(entry);
        entry.summary.status = ThreadRuntimeStatus::Evicted;
        entry.summary.last_reason = Some(reason);
        entry.summary.estimated_memory_bytes = 0;
        entry.slot_permit = None;
        let runtime = entry.summary.clone();
        Self::refresh_peaks(&mut store);
        let snapshot = Self::collect_metrics_from_store(max_threads, &store);
        Some((runtime, snapshot, shutdown))
    }

    async fn summarize_thread_history(thread: &Arc<RwLock<argus_agent::Thread>>) -> String {
        const SUMMARY_LIMIT: usize = 4000;

        let summary = {
            let guard = thread.read().await;
            guard
                .history_iter()
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
                .find_map(|message| match message {
                    ChatMessage {
                        role: Role::Assistant,
                        content,
                        ..
                    } if !content.is_empty() => Some(content.clone()),
                    _ => None,
                })
        };

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

    async fn thread_display_label(&self, thread_id: &ThreadId) -> String {
        let Some(thread) = self.loaded_thread(thread_id) else {
            return format!("Thread {}", thread_id);
        };

        let guard = thread.read().await;
        guard.agent_record().display_name.clone()
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

    async fn await_job_turn_result(
        &self,
        execution_thread_id: ThreadId,
        thread: &Arc<RwLock<argus_agent::Thread>>,
        mut runtime_rx: broadcast::Receiver<ThreadEvent>,
        fallback_job_id: String,
        cancellation: TurnCancellation,
    ) -> ThreadJobResult {
        let (agent_id, agent_display_name, agent_description) = {
            let guard = thread.read().await;
            let agent_record = guard.agent_record();
            (
                agent_record.id,
                agent_record.display_name.clone(),
                agent_record.description.clone(),
            )
        };

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
                    return Self::failure_result(
                        fallback_job_id,
                        agent_id,
                        agent_display_name,
                        agent_description,
                        message,
                    );
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
            return Self::failure_result(
                fallback_job_id,
                agent_id,
                agent_display_name,
                agent_description,
                message,
            );
        }

        ThreadJobResult {
            job_id: fallback_job_id,
            success: true,
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
            message,
            token_usage: None,
            agent_id,
            agent_display_name,
            agent_description,
        }
    }

    async fn recover_and_validate_metadata(
        base_dir: &Path,
        expected_thread_id: ThreadId,
        expected_kind: ThreadTraceKind,
    ) -> Result<ThreadTraceMetadata, JobError> {
        let metadata = recover_thread_metadata(base_dir)
            .await
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        if metadata.thread_id != expected_thread_id {
            return Err(JobError::ExecutionFailed(format!(
                "{expected_kind:?} trace metadata for {expected_thread_id} resolved to {}",
                metadata.thread_id
            )));
        }
        if metadata.kind != expected_kind {
            return Err(JobError::ExecutionFailed(format!(
                "thread {expected_thread_id} is not recorded as {expected_kind:?}"
            )));
        }
        Ok(metadata)
    }

    fn sync_relationship_cache(&self, metadata: &ThreadTraceMetadata) {
        let mut store = self.store.lock().expect("thread-pool mutex poisoned");

        if let Some(job_id) = metadata.job_id.as_deref() {
            store
                .job_bindings
                .insert(job_id.to_string(), metadata.thread_id);
        }

        if let Some(parent_thread_id) = metadata.parent_thread_id {
            store
                .parent_thread_by_child
                .insert(metadata.thread_id, parent_thread_id);
            let children = store
                .child_threads_by_parent
                .entry(parent_thread_id)
                .or_default();
            if !children.contains(&metadata.thread_id) {
                children.push(metadata.thread_id);
            }
        }
    }

    fn build_thread_config(
        base_dir: PathBuf,
        model_name: String,
    ) -> Result<argus_agent::ThreadConfig, JobError> {
        let trace_cfg = TraceConfig::new(true, base_dir).with_model(Some(model_name));
        let mut turn_config = TurnConfig::new();
        turn_config.trace_config = Some(trace_cfg);
        ThreadConfigBuilder::default()
            .turn_config(turn_config)
            .build()
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))
    }

    async fn hydrate_turn_log_state(
        thread: &Arc<RwLock<argus_agent::Thread>>,
        base_dir: &Path,
        updated_at: &str,
    ) -> Result<(), JobError> {
        let updated_at = chrono::DateTime::parse_from_rfc3339(updated_at)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now());
        let recovered = recover_thread_log_state(base_dir)
            .await
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        if recovered.turn_count() > 0 {
            thread
                .write()
                .await
                .hydrate_from_turn_log_state(recovered, updated_at);
        }
        Ok(())
    }

    pub(crate) async fn recover_job_thread_metadata(
        &self,
        thread_id: ThreadId,
    ) -> Result<Option<ThreadTraceMetadata>, JobError> {
        let base_dir =
            match find_job_thread_base_dir(&self.chat_runtime_config.trace_dir, thread_id).await {
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
        let metadata =
            Self::recover_and_validate_metadata(&base_dir, thread_id, ThreadTraceKind::Job).await?;
        self.sync_relationship_cache(&metadata);
        Ok(Some(metadata))
    }

    async fn trace_base_dir_for_thread(&self, thread_id: ThreadId) -> Result<PathBuf, JobError> {
        if let Some(thread) = self.loaded_thread(&thread_id) {
            return thread.read().await.trace_base_dir().ok_or_else(|| {
                JobError::ExecutionFailed(format!(
                    "thread {} does not expose a trace directory",
                    thread_id
                ))
            });
        }

        if let Some(persistence) = &self.persistence
            && let Some(thread_record) = persistence
                .thread_repository
                .get_thread(&thread_id)
                .await
                .map_err(|err| {
                    JobError::ExecutionFailed(format!("failed to load thread record: {err}"))
                })?
            && let Some(session_id) = thread_record.session_id
        {
            return Ok(chat_thread_base_dir(
                &self.chat_runtime_config.trace_dir,
                session_id,
                thread_id,
            ));
        }

        find_job_thread_base_dir(&self.chat_runtime_config.trace_dir, thread_id)
            .await
            .map_err(|_| {
                JobError::ExecutionFailed(format!("thread {} trace directory not found", thread_id))
            })
    }

    async fn resolve_provider_with_fallback(
        &self,
        provider_id: ProviderId,
        model: Option<&str>,
    ) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
        match model {
            Some(model) => {
                match self
                    .provider_resolver
                    .resolve_with_model(provider_id, model)
                    .await
                {
                    Ok(provider) => Ok(provider),
                    Err(_) => self.provider_resolver.resolve(provider_id).await,
                }
            }
            None => self.provider_resolver.resolve(provider_id).await,
        }
    }

    async fn build_chat_thread(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Result<Arc<RwLock<argus_agent::Thread>>, JobError> {
        let persistence = self.persistence.as_ref().ok_or_else(|| {
            JobError::ExecutionFailed("thread pool persistence is not configured".to_string())
        })?;
        let thread_record = persistence
            .thread_repository
            .get_thread_in_session(&thread_id, &session_id)
            .await
            .map_err(|err| {
                JobError::ExecutionFailed(format!("failed to load thread record: {err}"))
            })?
            .ok_or_else(|| JobError::ExecutionFailed(format!("thread {} not found", thread_id)))?;
        let base_dir =
            chat_thread_base_dir(&self.chat_runtime_config.trace_dir, session_id, thread_id);
        let metadata =
            Self::recover_and_validate_metadata(&base_dir, thread_id, ThreadTraceKind::ChatRoot)
                .await?;
        let agent_record = metadata.agent_snapshot.clone();
        let provider_id = ProviderId::new(thread_record.provider_id.into_inner());
        let requested_model = thread_record
            .model_override
            .clone()
            .unwrap_or_else(|| format!("provider-{}", provider_id.inner()));
        let provider = self
            .resolve_provider_with_fallback(provider_id, thread_record.model_override.as_deref())
            .await;
        let provider = match provider {
            Ok(provider) => provider,
            Err(error) => {
                tracing::warn!(
                    thread_id = %thread_id,
                    provider_id = %provider_id,
                    error = %error,
                    "Failed to resolve chat provider, using unavailable placeholder provider"
                );
                Arc::new(UnavailableChatProvider::new(
                    requested_model,
                    format!("failed to resolve provider: {error}"),
                )) as Arc<dyn argus_protocol::LlmProvider>
            }
        };

        let config =
            Self::build_thread_config(base_dir.clone(), provider.model_name().to_string())?;
        let thread_builder = ThreadBuilder::new()
            .id(thread_id)
            .session_id(session_id)
            .agent_record(Arc::new(agent_record))
            .title(thread_record.title.clone())
            .provider(provider.clone())
            .tool_manager(self.tool_manager.clone())
            .compactor(Arc::new(LlmThreadCompactor::new(provider)));
        let plan_store = FilePlanStore::new(base_dir.clone());
        let thread = thread_builder
            .plan_store(plan_store)
            .config(config)
            .build()
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        let thread = Arc::new(RwLock::new(thread));
        self.sync_relationship_cache(&metadata);

        Self::hydrate_turn_log_state(&thread, &base_dir, &thread_record.updated_at).await?;

        Ok(thread)
    }

    async fn build_job_thread(
        &self,
        request: &ThreadPoolJobRequest,
        thread_id: ThreadId,
    ) -> Result<Arc<RwLock<argus_agent::Thread>>, JobError> {
        let thread_record = if let Some(persistence) = &self.persistence {
            persistence
                .thread_repository
                .get_thread(&thread_id)
                .await
                .map_err(|err| {
                    JobError::ExecutionFailed(format!("failed to load thread record: {err}"))
                })?
        } else {
            None
        };
        let base_dir = find_job_thread_base_dir(&self.chat_runtime_config.trace_dir, thread_id)
            .await
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        let metadata =
            Self::recover_and_validate_metadata(&base_dir, thread_id, ThreadTraceKind::Job).await?;
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

        let config =
            Self::build_thread_config(base_dir.clone(), provider.model_name().to_string())?;
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
            .tool_manager(self.tool_manager.clone())
            .compactor(Arc::new(LlmThreadCompactor::new(provider)))
            .plan_store(plan_store)
            .config(config);
        if let Some(resolver) = self
            .mcp_tool_resolver
            .lock()
            .expect("mcp resolver mutex poisoned")
            .clone()
        {
            builder = builder.mcp_tool_resolver(resolver);
        }
        let thread = builder
            .build()
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        let thread = Arc::new(RwLock::new(thread));
        self.sync_relationship_cache(&metadata);

        if let Some(thread_record) = thread_record {
            Self::hydrate_turn_log_state(&thread, &base_dir, &thread_record.updated_at).await?;
        }

        Ok(thread)
    }

    fn job_runtime_session_id(thread_id: ThreadId) -> SessionId {
        SessionId(*thread_id.inner())
    }

    async fn estimate_thread_memory(thread: &Arc<RwLock<argus_agent::Thread>>) -> u64 {
        let guard = thread.read().await;
        let history_bytes = guard
            .history_iter()
            .map(|message| message.content.len() as u64)
            .sum::<u64>();
        let plan_bytes = guard.plan().len() as u64 * 128;
        history_bytes + plan_bytes + u64::from(guard.token_count())
    }

    async fn await_runtime_idle_settle(thread: &Arc<RwLock<argus_agent::Thread>>) -> bool {
        for _ in 0..64 {
            if !thread.read().await.is_turn_running() {
                return true;
            }
            tokio::task::yield_now().await;
        }

        !thread.read().await.is_turn_running()
    }

    async fn persist_thread_stats(
        &self,
        thread_id: &ThreadId,
        thread: &Arc<RwLock<argus_agent::Thread>>,
    ) {
        Self::persist_thread_stats_with_persistence(
            self.persistence.as_ref(),
            thread_id,
            thread,
            "job thread",
        )
        .await;
    }

    async fn persist_thread_stats_with_persistence(
        persistence: Option<&ThreadPoolPersistence>,
        thread_id: &ThreadId,
        thread: &Arc<RwLock<argus_agent::Thread>>,
        runtime_label: &str,
    ) {
        let Some(persistence) = persistence else {
            return;
        };
        let (token_count, turn_count) = {
            let guard = thread.read().await;
            (guard.token_count(), guard.turn_count())
        };
        if let Err(error) = persistence
            .thread_repository
            .update_thread_stats(thread_id, token_count, turn_count)
            .await
        {
            tracing::warn!(
                thread_id = %thread_id,
                runtime_label,
                error = %error,
                "Failed to persist runtime stats after idle"
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
        let Some(persistence) = &self.persistence else {
            return Ok(());
        };
        persistence
            .job_repository
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
        let Some(persistence) = &self.persistence else {
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
        if let Err(error) = persistence
            .job_repository
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
        } else {
            JobStatus::Failed
        };
        if let Err(error) = persistence
            .job_repository
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
        request: &ThreadPoolJobRequest,
        now: &str,
    ) -> Result<ThreadId, JobError> {
        if self.persistence.is_none() {
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

        let (existing_job, existing_thread_id, existing_thread) = if let Some(persistence) =
            &self.persistence
        {
            let job_id = JobId::new(request.job_id.clone());
            let existing_job = persistence
                .job_repository
                .get(&job_id)
                .await
                .map_err(|err| {
                    JobError::ExecutionFailed(format!("failed to load job record: {err}"))
                })?;
            let existing_thread_id = existing_job.as_ref().and_then(|job| job.thread_id);
            let existing_thread = if let Some(thread_id) = existing_thread_id {
                persistence
                    .thread_repository
                    .get_thread(&thread_id)
                    .await
                    .map_err(|err| {
                        JobError::ExecutionFailed(format!("failed to load thread record: {err}"))
                    })?
            } else {
                None
            };
            (existing_job, existing_thread_id, existing_thread)
        } else {
            (None, None, None)
        };

        let thread_id = existing_thread_id.unwrap_or_else(ThreadId::new);
        let should_cleanup_trace_dir = existing_thread_id.is_none();
        let default_base_dir = child_thread_base_dir(&parent_base_dir, thread_id);
        let (base_dir, _existing_child_metadata) = if existing_thread_id.is_some() {
            let existing_base_dir =
                find_job_thread_base_dir(&self.chat_runtime_config.trace_dir, thread_id)
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
            (existing_base_dir, Some(metadata))
        } else {
            (default_base_dir, None)
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
        self.sync_relationship_cache(&metadata);

        let Some(persistence) = &self.persistence else {
            return Ok(thread_id);
        };

        let template_provider_id = agent_record
            .provider_id
            .map(|id| argus_protocol::LlmProviderId::new(id.inner()));
        let provider_id = match template_provider_id {
            Some(provider_id) => provider_id,
            None => persistence
                .provider_repository
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
        if let Err(err) = persistence
            .thread_repository
            .upsert_thread(&thread_record)
            .await
        {
            if should_cleanup_trace_dir {
                Self::cleanup_trace_dir(&base_dir).await;
            }
            return Err(JobError::ExecutionFailed(format!(
                "failed to persist thread record: {err}"
            )));
        }

        let job_id = JobId::new(request.job_id.clone());
        if existing_job.is_some() {
            if existing_thread_id.is_none()
                && let Err(err) =
                    Self::persist_existing_job_binding(persistence, &job_id, thread_id).await
            {
                if should_cleanup_trace_dir {
                    Self::cleanup_trace_dir(&base_dir).await;
                }
                return Err(
                    Self::rollback_thread_record(persistence, thread_id, format!("{err}")).await,
                );
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

        if let Err(err) = persistence.job_repository.create(&job_record).await {
            if should_cleanup_trace_dir {
                Self::cleanup_trace_dir(&base_dir).await;
            }
            return Err(Self::rollback_thread_record(
                persistence,
                thread_id,
                format!("failed to create job record: {err}"),
            )
            .await);
        }

        Ok(thread_id)
    }

    async fn cleanup_trace_dir(base_dir: &Path) {
        match tokio::fs::remove_dir_all(base_dir).await {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::warn!(
                    path = %base_dir.display(),
                    error = %error,
                    "failed to clean up thread trace directory"
                );
            }
        }
    }

    async fn persist_existing_job_binding(
        persistence: &ThreadPoolPersistence,
        job_id: &JobId,
        thread_id: ThreadId,
    ) -> Result<(), JobError> {
        persistence
            .job_repository
            .update_thread_id(job_id, &thread_id)
            .await
            .map_err(|err| {
                JobError::ExecutionFailed(format!("failed to persist job-thread binding: {err}"))
            })
    }

    async fn rollback_thread_record(
        persistence: &ThreadPoolPersistence,
        thread_id: ThreadId,
        message: String,
    ) -> JobError {
        match persistence
            .thread_repository
            .delete_thread(&thread_id)
            .await
        {
            Ok(_) => JobError::ExecutionFailed(message),
            Err(cleanup_err) => JobError::ExecutionFailed(format!(
                "{message}; failed to roll back thread record: {cleanup_err}"
            )),
        }
    }

    #[cfg(test)]
    pub(crate) fn test_pool() -> Self {
        use argus_protocol::{LlmProvider, ProviderId};
        use argus_repository::ArgusSqlite;
        use argus_repository::traits::AgentRepository;
        use async_trait::async_trait;
        use sqlx::SqlitePool;

        #[derive(Debug)]
        struct DummyProviderResolver;

        #[async_trait]
        impl ProviderResolver for DummyProviderResolver {
            async fn resolve(
                &self,
                _id: ProviderId,
            ) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
                unreachable!("resolver should not be called in thread-pool state tests");
            }

            async fn default_provider(&self) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
                unreachable!("resolver should not be called in thread-pool state tests");
            }

            async fn resolve_with_model(
                &self,
                _id: ProviderId,
                _model: &str,
            ) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
                unreachable!("resolver should not be called in thread-pool state tests");
            }
        }

        let pool = SqlitePool::connect_lazy("sqlite::memory:")
            .expect("lazy sqlite pool should build for tests");
        let sqlite = Arc::new(ArgusSqlite::new(pool));
        Self::new(
            Arc::new(TemplateManager::new(
                sqlite.clone() as Arc<dyn AgentRepository>,
                sqlite,
            )),
            Arc::new(DummyProviderResolver),
            Arc::new(ToolManager::new()),
            std::env::temp_dir().join("argus-thread-pool-tests"),
        )
    }
}

#[cfg(test)]
pub(crate) async fn assert_enqueue_job_creates_binding_and_updates_metrics() {
    let pool = ThreadPool::test_pool();
    let thread_id = pool
        .enqueue_job(test_request("job-1"))
        .await
        .expect("enqueue should succeed");

    let snapshot = pool.collect_metrics();
    assert_eq!(snapshot.queued_threads, 1);
    assert_eq!(pool.get_thread_binding("job-1"), Some(thread_id));
}

#[cfg(test)]
fn test_request(job_id: &str) -> ThreadPoolJobRequest {
    ThreadPoolJobRequest {
        originating_thread_id: argus_protocol::ThreadId::new(),
        job_id: job_id.to_string(),
        agent_id: AgentId::new(7),
        prompt: "run test".to_string(),
        context: None,
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use super::*;
    use argus_agent::{Compactor, ThreadBuilder};
    use argus_protocol::llm::{
        CompletionRequest, CompletionResponse, FinishReason, LlmError, LlmEventStream, ToolCall,
        ToolDefinition,
    };
    use argus_protocol::{
        AgentRecord, NamedTool, ProviderId, ResolvedMcpTools, ThinkingConfig, ToolError,
        ToolExecutionContext,
    };
    use argus_repository::ArgusSqlite;
    use argus_repository::error::DbError;
    use argus_repository::migrate;
    use argus_repository::traits::{
        AgentRepository, JobRepository, LlmProviderRepository, SessionRepository, ThreadRepository,
    };
    use argus_repository::types::{MessageId, MessageRecord};
    use argus_template::TemplateManager;
    use async_trait::async_trait;
    use rust_decimal::Decimal;
    use sqlx::SqlitePool;
    use tokio::time::{Duration, sleep, timeout};

    struct NoopCompactor;

    #[async_trait]
    impl Compactor for NoopCompactor {
        async fn compact(
            &self,
            _messages: &[argus_protocol::llm::ChatMessage],
            _token_count: u32,
        ) -> Result<Option<argus_agent::CompactResult>, argus_agent::CompactError> {
            Ok(None)
        }

        fn name(&self) -> &'static str {
            "noop"
        }
    }

    #[derive(Debug)]
    struct FixedProvider;

    #[async_trait]
    impl argus_protocol::LlmProvider for FixedProvider {
        fn model_name(&self) -> &str {
            "job-test"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> std::result::Result<CompletionResponse, LlmError> {
            Err(LlmError::RequestFailed {
                provider: "job-test".to_string(),
                reason: "not used in job snapshot test".to_string(),
            })
        }

        async fn stream_complete(
            &self,
            _request: CompletionRequest,
        ) -> std::result::Result<LlmEventStream, LlmError> {
            Err(LlmError::UnsupportedCapability {
                provider: "job-test".to_string(),
                capability: "stream_complete".to_string(),
            })
        }
    }

    #[derive(Debug)]
    struct PendingStreamProvider;

    #[async_trait]
    impl argus_protocol::LlmProvider for PendingStreamProvider {
        fn model_name(&self) -> &str {
            "pending-job-test"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> std::result::Result<CompletionResponse, LlmError> {
            Err(LlmError::UnsupportedCapability {
                provider: "pending-job-test".to_string(),
                capability: "complete".to_string(),
            })
        }

        async fn stream_complete(
            &self,
            _request: CompletionRequest,
        ) -> std::result::Result<LlmEventStream, LlmError> {
            Ok(Box::pin(futures_util::stream::pending()))
        }
    }

    struct FixedProviderResolver {
        provider: Arc<dyn argus_protocol::LlmProvider>,
    }

    impl std::fmt::Debug for FixedProviderResolver {
        fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.debug_struct("FixedProviderResolver").finish()
        }
    }

    #[async_trait]
    impl argus_protocol::ProviderResolver for FixedProviderResolver {
        async fn resolve(
            &self,
            _id: ProviderId,
        ) -> argus_protocol::Result<Arc<dyn argus_protocol::LlmProvider>> {
            Ok(Arc::clone(&self.provider))
        }

        async fn default_provider(
            &self,
        ) -> argus_protocol::Result<Arc<dyn argus_protocol::LlmProvider>> {
            Ok(Arc::clone(&self.provider))
        }

        async fn resolve_with_model(
            &self,
            _id: ProviderId,
            _model: &str,
        ) -> argus_protocol::Result<Arc<dyn argus_protocol::LlmProvider>> {
            Ok(Arc::clone(&self.provider))
        }
    }

    struct MockToolCallProvider {
        responses: StdMutex<Vec<CompletionResponse>>,
    }

    impl MockToolCallProvider {
        fn new(responses: Vec<CompletionResponse>) -> Self {
            Self {
                responses: StdMutex::new(responses.into_iter().rev().collect()),
            }
        }
    }

    #[async_trait]
    impl argus_protocol::LlmProvider for MockToolCallProvider {
        fn model_name(&self) -> &str {
            "job-mcp-test"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> std::result::Result<CompletionResponse, LlmError> {
            self.responses
                .lock()
                .expect("mock provider mutex poisoned")
                .pop()
                .ok_or_else(|| LlmError::RequestFailed {
                    provider: "job-mcp-test".to_string(),
                    reason: "no more mock responses".to_string(),
                })
        }

        async fn stream_complete(
            &self,
            _request: CompletionRequest,
        ) -> std::result::Result<LlmEventStream, LlmError> {
            Err(LlmError::UnsupportedCapability {
                provider: "job-mcp-test".to_string(),
                capability: "stream_complete".to_string(),
            })
        }
    }

    struct McpEchoTool;

    #[async_trait]
    impl NamedTool for McpEchoTool {
        fn name(&self) -> &str {
            "mcp_echo"
        }

        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "mcp_echo".to_string(),
                description: "Echo a string through the MCP resolver".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "message": {
                            "type": "string"
                        }
                    },
                    "required": ["message"]
                }),
            }
        }

        async fn execute(
            &self,
            args: serde_json::Value,
            _ctx: Arc<ToolExecutionContext>,
        ) -> Result<serde_json::Value, ToolError> {
            Ok(serde_json::json!({
                "echoed": args
                    .get("message")
                    .and_then(|value| value.as_str())
                    .unwrap_or_default(),
            }))
        }
    }

    struct FixedMcpResolver;

    #[async_trait]
    impl McpToolResolver for FixedMcpResolver {
        async fn resolve_for_agent(
            &self,
            _agent_id: AgentId,
        ) -> argus_protocol::Result<ResolvedMcpTools> {
            Ok(ResolvedMcpTools::new(
                vec![Arc::new(McpEchoTool)],
                Vec::new(),
            ))
        }
    }

    fn routing_test_agent_record(agent_id: AgentId) -> AgentRecord {
        AgentRecord {
            id: agent_id,
            display_name: "Routing Test Agent".to_string(),
            description: "Used to verify mailbox delivery routing".to_string(),
            version: "1.0.0".to_string(),
            provider_id: Some(ProviderId::new(1)),
            model_id: Some("job-test".to_string()),
            system_prompt: "You route thread messages.".to_string(),
            tool_names: vec![],
            subagent_names: vec![],
            max_tokens: None,
            temperature: None,
            thinking_config: Some(ThinkingConfig::disabled()),
        }
    }

    async fn setup_persisted_chat_runtime(
        provider: Arc<dyn argus_protocol::LlmProvider>,
    ) -> (PathBuf, ThreadPool, SessionId, ThreadId, AgentId) {
        let trace_dir =
            std::env::temp_dir().join(format!("argus-thread-pool-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&trace_dir).expect("trace dir should exist");
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("sqlite memory pool should connect");
        migrate(&pool).await.expect("migration should succeed");
        let sqlite = Arc::new(ArgusSqlite::new(pool));

        let template_manager = Arc::new(TemplateManager::new(
            sqlite.clone() as Arc<dyn AgentRepository>,
            sqlite.clone(),
        ));
        let agent_id = AgentId::new(17);
        let agent_record = routing_test_agent_record(agent_id);
        template_manager
            .upsert(agent_record.clone())
            .await
            .expect("template upsert should succeed");

        let thread_pool = ThreadPool::with_persistence(
            template_manager,
            Arc::new(FixedProviderResolver { provider }),
            Arc::new(ToolManager::new()),
            trace_dir.clone(),
            Some(ThreadPoolPersistence::new(
                sqlite.clone() as Arc<dyn JobRepository>,
                sqlite.clone() as Arc<dyn ThreadRepository>,
                sqlite.clone() as Arc<dyn LlmProviderRepository>,
            )),
        );

        let session_id = SessionId::new();
        let thread_id = ThreadId::new();
        SessionRepository::create(sqlite.as_ref(), &session_id, "routing-session")
            .await
            .expect("session should persist");
        sqlite
            .upsert_thread(&ThreadRecord {
                id: thread_id,
                provider_id: argus_protocol::LlmProviderId::new(1),
                title: Some("routing-thread".to_string()),
                token_count: 0,
                turn_count: 0,
                session_id: Some(session_id),
                template_id: Some(RepoAgentId::new(agent_id.inner())),
                model_override: Some("job-test".to_string()),
                created_at: Utc::now().to_rfc3339(),
                updated_at: Utc::now().to_rfc3339(),
            })
            .await
            .expect("thread record should persist");

        let base_dir = chat_thread_base_dir(&trace_dir, session_id, thread_id);
        persist_thread_metadata(
            &base_dir,
            &ThreadTraceMetadata {
                thread_id,
                kind: ThreadTraceKind::ChatRoot,
                root_session_id: Some(session_id),
                parent_thread_id: None,
                job_id: None,
                agent_snapshot: agent_record,
            },
        )
        .await
        .expect("chat metadata should persist");

        (trace_dir, thread_pool, session_id, thread_id, agent_id)
    }

    fn plain_mailbox_message(to_thread_id: ThreadId) -> MailboxMessage {
        MailboxMessage {
            id: Uuid::new_v4().to_string(),
            from_thread_id: ThreadId::new(),
            to_thread_id,
            from_label: "planner".to_string(),
            message_type: MailboxMessageType::Plain,
            text: "hello from planner".to_string(),
            timestamp: Utc::now().to_rfc3339(),
            read: false,
            summary: Some("routing".to_string()),
        }
    }

    fn job_result_mailbox_message(to_thread_id: ThreadId, agent_id: AgentId) -> MailboxMessage {
        MailboxMessage {
            id: Uuid::new_v4().to_string(),
            from_thread_id: ThreadId::new(),
            to_thread_id,
            from_label: "worker".to_string(),
            message_type: MailboxMessageType::JobResult {
                job_id: "job-routing".to_string(),
                success: true,
                token_usage: None,
                agent_id,
                agent_display_name: "Routing Worker".to_string(),
                agent_description: "Produces a routed result".to_string(),
            },
            text: "finished routed work".to_string(),
            timestamp: Utc::now().to_rfc3339(),
            read: false,
            summary: Some("job done".to_string()),
        }
    }

    async fn wait_until_thread_running(thread: &Arc<RwLock<argus_agent::Thread>>) {
        timeout(Duration::from_secs(5), async {
            loop {
                if thread.read().await.is_turn_running() {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("thread should start running");
    }

    struct FailingUpsertThreadRepository {
        inner: Arc<ArgusSqlite>,
    }

    #[async_trait]
    impl ThreadRepository for FailingUpsertThreadRepository {
        async fn upsert_thread(&self, _thread: &ThreadRecord) -> Result<(), DbError> {
            Err(DbError::QueryFailed {
                reason: "forced thread upsert failure for cleanup test".to_string(),
            })
        }

        async fn get_thread(&self, id: &ThreadId) -> Result<Option<ThreadRecord>, DbError> {
            self.inner.get_thread(id).await
        }

        async fn list_threads(&self, limit: u32) -> Result<Vec<ThreadRecord>, DbError> {
            self.inner.list_threads(limit).await
        }

        async fn list_threads_in_session(
            &self,
            session_id: &SessionId,
        ) -> Result<Vec<ThreadRecord>, DbError> {
            self.inner.list_threads_in_session(session_id).await
        }

        async fn delete_thread(&self, id: &ThreadId) -> Result<bool, DbError> {
            self.inner.delete_thread(id).await
        }

        async fn delete_threads_in_session(&self, session_id: &SessionId) -> Result<u64, DbError> {
            self.inner.delete_threads_in_session(session_id).await
        }

        async fn add_message(&self, message: &MessageRecord) -> Result<MessageId, DbError> {
            self.inner.add_message(message).await
        }

        async fn get_messages(&self, thread_id: &ThreadId) -> Result<Vec<MessageRecord>, DbError> {
            self.inner.get_messages(thread_id).await
        }

        async fn get_recent_messages(
            &self,
            thread_id: &ThreadId,
            limit: u32,
        ) -> Result<Vec<MessageRecord>, DbError> {
            self.inner.get_recent_messages(thread_id, limit).await
        }

        async fn delete_messages_before(
            &self,
            thread_id: &ThreadId,
            seq: u32,
        ) -> Result<u64, DbError> {
            self.inner.delete_messages_before(thread_id, seq).await
        }

        async fn update_thread_stats(
            &self,
            id: &ThreadId,
            token_count: u32,
            turn_count: u32,
        ) -> Result<(), DbError> {
            self.inner
                .update_thread_stats(id, token_count, turn_count)
                .await
        }

        async fn rename_thread(
            &self,
            id: &ThreadId,
            session_id: &SessionId,
            title: Option<&str>,
        ) -> Result<bool, DbError> {
            self.inner.rename_thread(id, session_id, title).await
        }

        async fn update_thread_model(
            &self,
            id: &ThreadId,
            session_id: &SessionId,
            provider_id: argus_protocol::LlmProviderId,
            model_override: Option<&str>,
        ) -> Result<bool, DbError> {
            self.inner
                .update_thread_model(id, session_id, provider_id, model_override)
                .await
        }

        async fn get_thread_in_session(
            &self,
            thread_id: &ThreadId,
            session_id: &SessionId,
        ) -> Result<Option<ThreadRecord>, DbError> {
            self.inner
                .get_thread_in_session(thread_id, session_id)
                .await
        }
    }

    #[tokio::test]
    async fn job_thread_rehydrates_from_trace_snapshot_instead_of_latest_template() {
        let trace_dir =
            std::env::temp_dir().join(format!("argus-thread-pool-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&trace_dir).expect("trace dir should exist");
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
        let original_agent_record = AgentRecord {
            id: agent_id,
            display_name: "Job Snapshot Agent".to_string(),
            description: "Original job template".to_string(),
            version: "1.0.0".to_string(),
            provider_id: Some(ProviderId::new(1)),
            model_id: Some("job-test".to_string()),
            system_prompt: "You are the original job snapshot agent.".to_string(),
            tool_names: vec![],
            subagent_names: vec![],
            max_tokens: None,
            temperature: None,
            thinking_config: Some(ThinkingConfig::disabled()),
        };
        template_manager
            .upsert(original_agent_record.clone())
            .await
            .expect("template upsert should succeed");

        let provider_resolver = Arc::new(FixedProviderResolver {
            provider: Arc::new(FixedProvider),
        });
        let thread_pool = ThreadPool::with_persistence(
            Arc::clone(&template_manager),
            provider_resolver,
            Arc::new(ToolManager::new()),
            trace_dir.clone(),
            Some(ThreadPoolPersistence::new(
                sqlite.clone() as Arc<dyn JobRepository>,
                sqlite.clone() as Arc<dyn ThreadRepository>,
                sqlite.clone() as Arc<dyn LlmProviderRepository>,
            )),
        );

        let parent_session_id = SessionId::new();
        let parent_thread_id = ThreadId::new();
        SessionRepository::create(sqlite.as_ref(), &parent_session_id, "parent")
            .await
            .expect("parent session should persist");
        sqlite
            .upsert_thread(&ThreadRecord {
                id: parent_thread_id,
                provider_id: argus_protocol::LlmProviderId::new(1),
                title: Some("parent".to_string()),
                token_count: 0,
                turn_count: 0,
                session_id: Some(parent_session_id),
                template_id: Some(RepoAgentId::new(agent_id.inner())),
                model_override: Some("job-test".to_string()),
                created_at: Utc::now().to_rfc3339(),
                updated_at: Utc::now().to_rfc3339(),
            })
            .await
            .expect("parent thread record should persist");
        let parent_base_dir = chat_thread_base_dir(&trace_dir, parent_session_id, parent_thread_id);
        persist_thread_metadata(
            &parent_base_dir,
            &ThreadTraceMetadata {
                thread_id: parent_thread_id,
                kind: ThreadTraceKind::ChatRoot,
                root_session_id: Some(parent_session_id),
                parent_thread_id: None,
                job_id: None,
                agent_snapshot: original_agent_record.clone(),
            },
        )
        .await
        .expect("parent metadata should persist");

        let request = ThreadPoolJobRequest {
            originating_thread_id: parent_thread_id,
            job_id: "job-snapshot".to_string(),
            agent_id,
            prompt: "run test".to_string(),
            context: None,
        };
        let thread_id = thread_pool
            .persist_binding(&request, &Utc::now().to_string())
            .await
            .expect("job binding should persist");

        let snapshot_path = parent_base_dir
            .join(thread_id.to_string())
            .join("thread.json");
        let snapshot_content = tokio::fs::read_to_string(&snapshot_path)
            .await
            .expect("job thread snapshot should exist");
        let snapshot: ThreadTraceMetadata =
            serde_json::from_str(&snapshot_content).expect("job snapshot should deserialize");
        assert_eq!(
            snapshot.agent_snapshot.system_prompt,
            "You are the original job snapshot agent."
        );
        assert_eq!(snapshot.parent_thread_id, Some(parent_thread_id));
        assert_eq!(snapshot.root_session_id, Some(parent_session_id));
        assert_eq!(snapshot.job_id.as_deref(), Some("job-snapshot"));
        let parent_content = tokio::fs::read_to_string(parent_base_dir.join("thread.json"))
            .await
            .expect("parent thread metadata should exist");
        let parent_snapshot: ThreadTraceMetadata =
            serde_json::from_str(&parent_content).expect("parent metadata should deserialize");
        assert_eq!(parent_snapshot.job_id, None);

        template_manager
            .upsert(AgentRecord {
                id: agent_id,
                display_name: "Job Snapshot Agent Updated".to_string(),
                description: "Updated job template".to_string(),
                version: "2.0.0".to_string(),
                provider_id: Some(ProviderId::new(1)),
                model_id: Some("job-test-v2".to_string()),
                system_prompt: "You are the mutated job snapshot agent.".to_string(),
                tool_names: vec![],
                subagent_names: vec![],
                max_tokens: None,
                temperature: None,
                thinking_config: Some(ThinkingConfig::disabled()),
            })
            .await
            .expect("template update should succeed");

        let thread = thread_pool
            .build_job_thread(&request, thread_id)
            .await
            .expect("job thread should rebuild from trace snapshot");
        let guard = thread.read().await;
        assert_eq!(
            guard.agent_record().system_prompt,
            "You are the original job snapshot agent."
        );
        assert_eq!(guard.agent_record().display_name, "Job Snapshot Agent");
        drop(guard);
        assert_eq!(
            thread_pool.parent_thread_id(&thread_id),
            Some(parent_thread_id)
        );
        assert_eq!(
            thread_pool.child_thread_ids(&parent_thread_id),
            vec![thread_id]
        );
        let recovered_children = thread_pool
            .recover_child_jobs(parent_thread_id)
            .await
            .expect("child jobs should recover from trace");
        assert_eq!(
            recovered_children,
            vec![RecoveredChildJob {
                thread_id,
                job_id: "job-snapshot".to_string(),
            }]
        );
    }

    #[tokio::test]
    async fn deliver_mailbox_message_marks_chat_runtime_running_and_skips_plain_shadow_copy() {
        let (_temp_dir, thread_pool, session_id, thread_id, _agent_id) =
            setup_persisted_chat_runtime(Arc::new(PendingStreamProvider)).await;
        let thread = thread_pool
            .ensure_chat_runtime(session_id, thread_id)
            .await
            .expect("chat runtime should load");
        let estimated_memory_bytes = ThreadPool::estimate_thread_memory(&thread).await;
        assert!(
            thread_pool
                .transition_runtime_to_cooling(&thread_id, Some(estimated_memory_bytes))
                .is_some(),
            "loaded runtime should be movable to cooling"
        );
        assert_eq!(
            thread_pool
                .runtime_summary(&thread_id)
                .expect("runtime summary should exist")
                .status,
            ThreadRuntimeStatus::Cooling
        );

        thread_pool
            .deliver_mailbox_message(thread_id, plain_mailbox_message(thread_id))
            .await
            .expect("plain mailbox message should route through thread");
        wait_until_thread_running(&thread).await;

        assert_eq!(
            thread_pool
                .runtime_summary(&thread_id)
                .expect("runtime summary should exist")
                .status,
            ThreadRuntimeStatus::Running,
            "mailbox-routed chat turns should mark the runtime running before they execute"
        );
        assert!(
            thread_pool
                .store
                .lock()
                .expect("thread-pool mutex poisoned")
                .runtimes
                .get(&thread_id.to_string())
                .expect("runtime entry should exist")
                .queued_job_results
                .is_empty(),
            "plain scheduler messages should not leave behind a shared inbox shadow"
        );

        assert!(
            thread_pool.remove_runtime(&thread_id),
            "test cleanup should unload the pending runtime"
        );
    }

    #[tokio::test]
    async fn deliver_mailbox_job_result_rehydrates_evicted_chat_runtime_and_keeps_claimable_shadow()
    {
        let (_temp_dir, thread_pool, session_id, thread_id, agent_id) =
            setup_persisted_chat_runtime(Arc::new(PendingStreamProvider)).await;
        let thread = thread_pool
            .ensure_chat_runtime(session_id, thread_id)
            .await
            .expect("chat runtime should load");
        let estimated_memory_bytes = ThreadPool::estimate_thread_memory(&thread).await;
        assert!(
            thread_pool
                .transition_runtime_to_cooling(&thread_id, Some(estimated_memory_bytes))
                .is_some(),
            "loaded runtime should be movable to cooling"
        );
        assert!(
            thread_pool.evict_chat_if_idle(&thread_id).is_some(),
            "cooling runtime should evict"
        );
        assert!(
            thread_pool.loaded_thread(&thread_id).is_none(),
            "runtime should no longer be resident after eviction"
        );

        let message = job_result_mailbox_message(thread_id, agent_id);
        thread_pool
            .deliver_mailbox_message(thread_id, message.clone())
            .await
            .expect("job-result delivery should rehydrate and route the runtime");
        let thread = thread_pool
            .loaded_thread(&thread_id)
            .expect("delivery should rehydrate the evicted runtime");
        wait_until_thread_running(&thread).await;

        assert_eq!(
            thread_pool
                .runtime_summary(&thread_id)
                .expect("runtime summary should exist")
                .status,
            ThreadRuntimeStatus::Running,
            "rehydrated chat runtimes should become running before consuming mailbox-routed turns"
        );
        assert!(
            thread_pool
                .claim_queued_job_result(thread_id, "job-routing")
                .is_some(),
            "job-result routing should keep the consume-only shadow copy"
        );

        assert!(
            thread_pool.remove_runtime(&thread_id),
            "test cleanup should unload the rehydrated runtime"
        );
    }

    #[tokio::test]
    async fn job_thread_uses_configured_mcp_tool_resolver_for_turn_execution() {
        let trace_dir =
            std::env::temp_dir().join(format!("argus-thread-pool-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&trace_dir).expect("trace dir should exist");
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
            display_name: "Job MCP Agent".to_string(),
            description: "Uses MCP in job thread".to_string(),
            version: "1.0.0".to_string(),
            provider_id: Some(ProviderId::new(1)),
            model_id: Some("job-mcp-test".to_string()),
            system_prompt: "Use MCP when helpful.".to_string(),
            tool_names: vec![],
            subagent_names: vec![],
            max_tokens: None,
            temperature: None,
            thinking_config: Some(ThinkingConfig::disabled()),
        };
        template_manager
            .upsert(agent_record.clone())
            .await
            .expect("template upsert should succeed");

        let provider_resolver = Arc::new(FixedProviderResolver {
            provider: Arc::new(MockToolCallProvider::new(vec![
                CompletionResponse {
                    content: Some("Call MCP".to_string()),
                    reasoning_content: None,
                    tool_calls: vec![ToolCall {
                        id: "call-1".to_string(),
                        name: "mcp_echo".to_string(),
                        arguments: serde_json::json!({"message": "from job"}),
                    }],
                    input_tokens: 3,
                    output_tokens: 2,
                    finish_reason: FinishReason::ToolUse,
                    cache_read_input_tokens: 0,
                    cache_creation_input_tokens: 0,
                },
                CompletionResponse {
                    content: Some("Final answer after MCP".to_string()),
                    reasoning_content: None,
                    tool_calls: vec![],
                    input_tokens: 3,
                    output_tokens: 2,
                    finish_reason: FinishReason::Stop,
                    cache_read_input_tokens: 0,
                    cache_creation_input_tokens: 0,
                },
            ])),
        });
        let thread_pool = ThreadPool::with_persistence(
            Arc::clone(&template_manager),
            provider_resolver,
            Arc::new(ToolManager::new()),
            trace_dir.clone(),
            Some(ThreadPoolPersistence::new(
                sqlite.clone() as Arc<dyn JobRepository>,
                sqlite.clone() as Arc<dyn ThreadRepository>,
                sqlite.clone() as Arc<dyn LlmProviderRepository>,
            )),
        );
        thread_pool.set_mcp_tool_resolver(Some(Arc::new(FixedMcpResolver)));

        let parent_session_id = SessionId::new();
        let parent_thread_id = ThreadId::new();
        SessionRepository::create(sqlite.as_ref(), &parent_session_id, "parent")
            .await
            .expect("parent session should persist");
        sqlite
            .upsert_thread(&ThreadRecord {
                id: parent_thread_id,
                provider_id: argus_protocol::LlmProviderId::new(1),
                title: Some("parent".to_string()),
                token_count: 0,
                turn_count: 0,
                session_id: Some(parent_session_id),
                template_id: Some(RepoAgentId::new(agent_id.inner())),
                model_override: Some("job-mcp-test".to_string()),
                created_at: Utc::now().to_rfc3339(),
                updated_at: Utc::now().to_rfc3339(),
            })
            .await
            .expect("parent thread record should persist");
        let parent_base_dir = chat_thread_base_dir(&trace_dir, parent_session_id, parent_thread_id);
        persist_thread_metadata(
            &parent_base_dir,
            &ThreadTraceMetadata {
                thread_id: parent_thread_id,
                kind: ThreadTraceKind::ChatRoot,
                root_session_id: Some(parent_session_id),
                parent_thread_id: None,
                job_id: None,
                agent_snapshot: agent_record.clone(),
            },
        )
        .await
        .expect("parent metadata should persist");

        let request = ThreadPoolJobRequest {
            originating_thread_id: parent_thread_id,
            job_id: "job-mcp".to_string(),
            agent_id,
            prompt: "run test".to_string(),
            context: None,
        };
        let thread_id = thread_pool
            .persist_binding(&request, &Utc::now().to_string())
            .await
            .expect("job binding should persist");

        let thread = thread_pool
            .build_job_thread(&request, thread_id)
            .await
            .expect("job thread should build");
        let record = thread
            .write()
            .await
            .execute_turn("use mcp".to_string(), None, TurnCancellation::new())
            .await
            .expect("turn should succeed when MCP tool is resolved");

        let assistant_messages: Vec<_> = record
            .messages
            .iter()
            .filter(|message| message.role == Role::Assistant)
            .collect();
        assert!(
            assistant_messages.iter().any(|message| {
                message
                    .tool_calls
                    .as_ref()
                    .is_some_and(|tool_calls| !tool_calls.is_empty())
            }),
            "job thread should expose MCP tools to the LLM; messages={:?}",
            record
                .messages
                .iter()
                .map(|message| (
                    format!("{:?}", message.role),
                    message.content.clone(),
                    message.tool_calls.clone(),
                ))
                .collect::<Vec<_>>()
        );
        assert_eq!(
            assistant_messages
                .last()
                .map(|message| message.content.as_str()),
            Some("Final answer after MCP")
        );
    }

    #[tokio::test]
    async fn recover_child_jobs_rehydrates_sibling_relationships_without_parent_lists() {
        let trace_dir =
            std::env::temp_dir().join(format!("argus-thread-pool-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&trace_dir).expect("trace dir should exist");
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
            display_name: "Sibling Recovery Agent".to_string(),
            description: "Tests direct child recovery".to_string(),
            version: "1.0.0".to_string(),
            provider_id: Some(ProviderId::new(1)),
            model_id: Some("job-test".to_string()),
            system_prompt: "You recover child jobs from trace.".to_string(),
            tool_names: vec![],
            subagent_names: vec![],
            max_tokens: None,
            temperature: None,
            thinking_config: Some(ThinkingConfig::disabled()),
        };
        template_manager
            .upsert(agent_record.clone())
            .await
            .expect("template upsert should succeed");

        let provider_resolver = Arc::new(FixedProviderResolver {
            provider: Arc::new(FixedProvider),
        });
        let thread_pool = ThreadPool::with_persistence(
            Arc::clone(&template_manager),
            provider_resolver.clone(),
            Arc::new(ToolManager::new()),
            trace_dir.clone(),
            Some(ThreadPoolPersistence::new(
                sqlite.clone() as Arc<dyn JobRepository>,
                sqlite.clone() as Arc<dyn ThreadRepository>,
                sqlite.clone() as Arc<dyn LlmProviderRepository>,
            )),
        );

        let parent_session_id = SessionId::new();
        let parent_thread_id = ThreadId::new();
        SessionRepository::create(sqlite.as_ref(), &parent_session_id, "parent")
            .await
            .expect("parent session should persist");
        sqlite
            .upsert_thread(&ThreadRecord {
                id: parent_thread_id,
                provider_id: argus_protocol::LlmProviderId::new(1),
                title: Some("parent".to_string()),
                token_count: 0,
                turn_count: 0,
                session_id: Some(parent_session_id),
                template_id: Some(RepoAgentId::new(agent_id.inner())),
                model_override: Some("job-test".to_string()),
                created_at: Utc::now().to_rfc3339(),
                updated_at: Utc::now().to_rfc3339(),
            })
            .await
            .expect("parent thread record should persist");
        let parent_base_dir = chat_thread_base_dir(&trace_dir, parent_session_id, parent_thread_id);
        persist_thread_metadata(
            &parent_base_dir,
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
        .expect("parent metadata should persist");

        let first = thread_pool
            .persist_binding(
                &ThreadPoolJobRequest {
                    originating_thread_id: parent_thread_id,
                    job_id: "job-sibling-a".to_string(),
                    agent_id,
                    prompt: "run a".to_string(),
                    context: None,
                },
                &Utc::now().to_string(),
            )
            .await
            .expect("first child binding should persist");
        let second = thread_pool
            .persist_binding(
                &ThreadPoolJobRequest {
                    originating_thread_id: parent_thread_id,
                    job_id: "job-sibling-b".to_string(),
                    agent_id,
                    prompt: "run b".to_string(),
                    context: None,
                },
                &Utc::now().to_string(),
            )
            .await
            .expect("second child binding should persist");

        let fresh_pool = ThreadPool::with_persistence(
            template_manager,
            provider_resolver,
            Arc::new(ToolManager::new()),
            trace_dir,
            Some(ThreadPoolPersistence::new(
                sqlite.clone() as Arc<dyn JobRepository>,
                sqlite.clone() as Arc<dyn ThreadRepository>,
                sqlite as Arc<dyn LlmProviderRepository>,
            )),
        );

        let mut recovered = fresh_pool
            .recover_child_jobs(parent_thread_id)
            .await
            .expect("fresh pool should recover persisted direct children");
        recovered.sort_by(|left, right| left.job_id.cmp(&right.job_id));

        assert_eq!(
            recovered,
            vec![
                RecoveredChildJob {
                    thread_id: first,
                    job_id: "job-sibling-a".to_string(),
                },
                RecoveredChildJob {
                    thread_id: second,
                    job_id: "job-sibling-b".to_string(),
                },
            ]
        );
    }

    #[tokio::test]
    async fn persist_binding_cleans_trace_dir_before_returning_thread_record_errors() {
        let trace_dir =
            std::env::temp_dir().join(format!("argus-thread-pool-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&trace_dir).expect("trace dir should exist");
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
            display_name: "Cleanup Agent".to_string(),
            description: "Tests trace cleanup".to_string(),
            version: "1.0.0".to_string(),
            provider_id: Some(ProviderId::new(1)),
            model_id: Some("job-test".to_string()),
            system_prompt: "You clean up after failures.".to_string(),
            tool_names: vec![],
            subagent_names: vec![],
            max_tokens: None,
            temperature: None,
            thinking_config: Some(ThinkingConfig::disabled()),
        };
        template_manager
            .upsert(agent_record.clone())
            .await
            .expect("template upsert should succeed");

        let parent_session_id = SessionId::new();
        let parent_thread_id = ThreadId::new();
        SessionRepository::create(sqlite.as_ref(), &parent_session_id, "parent")
            .await
            .expect("parent session should persist");
        sqlite
            .upsert_thread(&ThreadRecord {
                id: parent_thread_id,
                provider_id: argus_protocol::LlmProviderId::new(1),
                title: Some("parent".to_string()),
                token_count: 0,
                turn_count: 0,
                session_id: Some(parent_session_id),
                template_id: Some(RepoAgentId::new(agent_id.inner())),
                model_override: Some("job-test".to_string()),
                created_at: Utc::now().to_rfc3339(),
                updated_at: Utc::now().to_rfc3339(),
            })
            .await
            .expect("parent thread record should persist");
        let parent_base_dir = chat_thread_base_dir(&trace_dir, parent_session_id, parent_thread_id);
        persist_thread_metadata(
            &parent_base_dir,
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
        .expect("parent metadata should persist");

        let thread_pool = ThreadPool::with_persistence(
            template_manager,
            Arc::new(FixedProviderResolver {
                provider: Arc::new(FixedProvider),
            }),
            Arc::new(ToolManager::new()),
            trace_dir,
            Some(ThreadPoolPersistence::new(
                sqlite.clone() as Arc<dyn JobRepository>,
                Arc::new(FailingUpsertThreadRepository {
                    inner: sqlite.clone(),
                }) as Arc<dyn ThreadRepository>,
                sqlite as Arc<dyn LlmProviderRepository>,
            )),
        );

        let error = thread_pool
            .persist_binding(
                &ThreadPoolJobRequest {
                    originating_thread_id: parent_thread_id,
                    job_id: "job-cleanup".to_string(),
                    agent_id,
                    prompt: "run cleanup".to_string(),
                    context: None,
                },
                &Utc::now().to_string(),
            )
            .await
            .expect_err("thread record persistence should fail");
        assert!(
            error
                .to_string()
                .contains("failed to persist thread record"),
            "unexpected error: {error}"
        );

        let children = list_direct_child_threads(&parent_base_dir, parent_thread_id)
            .await
            .expect("direct child scan should succeed");
        assert!(
            children.is_empty(),
            "failed binding should not leave persisted child traces behind"
        );
    }

    #[tokio::test]
    async fn summarize_and_estimate_thread_memory_include_turn_checkpoint_messages() {
        let mut thread = ThreadBuilder::new()
            .provider(Arc::new(FixedProvider))
            .compactor(Arc::new(NoopCompactor))
            .agent_record(Arc::new(AgentRecord {
                id: AgentId::new(1),
                display_name: "job-agent".to_string(),
                description: "job-agent".to_string(),
                version: "1.0.0".to_string(),
                provider_id: Some(ProviderId::new(1)),
                model_id: None,
                system_prompt: String::new(),
                tool_names: vec![],
                subagent_names: vec![],
                max_tokens: None,
                temperature: None,
                thinking_config: Some(ThinkingConfig::disabled()),
            }))
            .session_id(SessionId::new())
            .build()
            .expect("thread should build");
        thread.hydrate_from_turn_log_state(
            argus_agent::turn_log_store::RecoveredThreadLogState {
                turns: vec![argus_agent::TurnRecord::turn_checkpoint(
                    1,
                    vec![
                        argus_protocol::llm::ChatMessage::user("compressed user intent"),
                        argus_protocol::llm::ChatMessage::assistant("compressed assistant result"),
                    ],
                    argus_protocol::TokenUsage {
                        input_tokens: 2,
                        output_tokens: 1,
                        total_tokens: 3,
                    },
                )],
            },
            Utc::now(),
        );
        let thread = Arc::new(tokio::sync::RwLock::new(thread));

        let summary = ThreadPool::summarize_thread_history(&thread).await;
        let estimated = ThreadPool::estimate_thread_memory(&thread).await;

        assert_eq!(summary, "compressed assistant result");
        assert!(estimated >= "compressed user intentcompressed assistant result".len() as u64);
    }
}
