//! ThreadPool for coordinating unified job and chat runtimes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use crate::error::JobError;
use crate::types::ThreadPoolJobRequest;
use argus_agent::config::ThreadConfigBuilder;
use argus_agent::thread_trace_store::{
    persist_thread_snapshot, recover_thread_snapshot, thread_base_dir,
};
use argus_agent::turn_log_store::recover_thread_log_state;
use argus_agent::{
    FilePlanStore, LlmCompactor, OnTurnComplete, ThreadBuilder, TraceConfig, TurnCancellation,
    TurnConfig,
};
use argus_protocol::llm::{
    ChatMessage, CompletionRequest, CompletionResponse, LlmError, LlmEventStream, Role,
};
use argus_protocol::{
    AgentId, LlmProvider, MailboxMessage, MailboxMessageType, ProviderId, ProviderResolver,
    SessionId, ThreadControlEvent, ThreadEvent, ThreadId, ThreadJobResult, ThreadPoolEventReason,
    ThreadPoolRuntimeKind, ThreadPoolRuntimeRef, ThreadPoolRuntimeSummary, ThreadPoolSnapshot,
    ThreadPoolState, ThreadRuntimeStatus,
};
use argus_repository::traits::{JobRepository, LlmProviderRepository, ThreadRepository};
use argus_repository::types::{
    AgentId as RepoAgentId, JobId, JobRecord, JobResult, JobStatus, JobType, ThreadRecord,
};
use argus_template::TemplateManager;
use argus_tool::ToolManager;
use chrono::Utc;
use rust_decimal::Decimal;
use tokio::sync::{Mutex as AsyncMutex, OwnedSemaphorePermit, RwLock, Semaphore, broadcast, mpsc};
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
    control_tx: Option<mpsc::UnboundedSender<ThreadControlEvent>>,
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
    control_tx: Option<mpsc::UnboundedSender<ThreadControlEvent>>,
    forwarder_abort: Option<AbortHandle>,
}

impl RuntimeShutdown {
    fn run(self) {
        if let Some(forwarder_abort) = self.forwarder_abort {
            forwarder_abort.abort();
        }
        if let Some(control_tx) = self.control_tx {
            let _ = control_tx.send(ThreadControlEvent::ShutdownRuntime);
        }
        drop(self.thread);
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
}

/// Coordinates job-thread bindings, runtime state transitions, and metrics.
pub struct ThreadPool {
    template_manager: Arc<TemplateManager>,
    provider_resolver: Arc<dyn ProviderResolver>,
    tool_manager: Arc<ToolManager>,
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
            chat_runtime_config: ChatRuntimeConfig { trace_dir },
            persistence,
            max_threads: DEFAULT_MAX_THREADS,
            resident_slots: Arc::new(Semaphore::new(DEFAULT_MAX_THREADS as usize)),
            admission_waiters: Arc::new(AtomicUsize::new(0)),
            store: Arc::new(StdMutex::new(ThreadPoolStore::default())),
        }
    }

    /// Register a chat thread in the unified pool without loading its runtime.
    pub fn register_chat_thread(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> broadcast::Receiver<ThreadEvent> {
        let runtime = ThreadPoolRuntimeRef {
            thread_id,
            kind: ThreadPoolRuntimeKind::Chat,
            session_id: Some(session_id),
            job_id: None,
        };
        self.upsert_runtime_summary(
            runtime,
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
                control_tx: entry.control_tx,
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
                (entry.summary.runtime.kind == ThreadPoolRuntimeKind::Chat)
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

    /// Return the current runtime summary for a thread.
    pub fn runtime_summary(&self, thread_id: &ThreadId) -> Option<ThreadPoolRuntimeSummary> {
        self.store
            .lock()
            .expect("thread-pool mutex poisoned")
            .runtimes
            .get(&thread_id.to_string())
            .map(|entry| entry.summary.clone())
    }

    /// Return unread queued mailbox messages for a loaded runtime.
    pub async fn unread_mailbox_messages(
        &self,
        thread_id: ThreadId,
    ) -> Result<Vec<MailboxMessage>, JobError> {
        let thread = self.loaded_thread(&thread_id).ok_or_else(|| {
            JobError::ExecutionFailed(format!("thread {} is not loaded", thread_id))
        })?;
        let mailbox = {
            let guard = thread.read().await;
            guard.mailbox()
        };

        Ok(mailbox.lock().await.unread_mailbox_messages())
    }

    /// Mark a queued mailbox message as read for a loaded runtime.
    pub async fn mark_mailbox_message_read(
        &self,
        thread_id: ThreadId,
        message_id: &str,
    ) -> Result<bool, JobError> {
        let thread = self.loaded_thread(&thread_id).ok_or_else(|| {
            JobError::ExecutionFailed(format!("thread {} is not loaded", thread_id))
        })?;
        let mailbox = {
            let guard = thread.read().await;
            guard.mailbox()
        };

        Ok(mailbox.lock().await.mark_mailbox_message_read(message_id))
    }

    /// Deliver a mailbox message to a runtime thread.
    pub async fn deliver_mailbox_message(
        &self,
        thread_id: ThreadId,
        message: MailboxMessage,
    ) -> Result<(), JobError> {
        let thread = match self.runtime_summary(&thread_id) {
            Some(summary) if summary.runtime.kind == ThreadPoolRuntimeKind::Chat => {
                let session_id = summary.runtime.session_id.ok_or_else(|| {
                    JobError::ExecutionFailed(format!(
                        "chat thread {} is missing a session binding",
                        thread_id
                    ))
                })?;
                self.ensure_chat_runtime(session_id, thread_id).await?
            }
            Some(_) => self.loaded_thread(&thread_id).ok_or_else(|| {
                JobError::ExecutionFailed(format!("thread {} is not loaded", thread_id))
            })?,
            None => {
                return Err(JobError::ExecutionFailed(format!(
                    "thread {} is not registered",
                    thread_id
                )));
            }
        };

        let mailbox = {
            let guard = thread.read().await;
            guard.mailbox()
        };
        mailbox
            .lock()
            .await
            .enqueue_mailbox_message(message.clone());
        {
            let guard = thread.read().await;
            let _ = guard.control_tx().send(ThreadControlEvent::MailboxUpdated);
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
        let runtime = ThreadPoolRuntimeRef {
            thread_id,
            kind: ThreadPoolRuntimeKind::Job,
            session_id: None,
            job_id: Some(request.job_id.clone()),
        };
        self.upsert_runtime_summary(
            runtime,
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

    /// Mark a queued runtime as running.
    pub fn mark_running(&self, job_id: &str) -> Option<ThreadId> {
        let thread_id = self.get_thread_binding(job_id)?;
        self.update_state(&thread_id, ThreadRuntimeStatus::Running, None)
    }

    /// Mark a runtime as cooling.
    pub fn mark_cooling(&self, job_id: &str) -> Option<ThreadId> {
        let thread_id = self.get_thread_binding(job_id)?;
        self.update_state(&thread_id, ThreadRuntimeStatus::Cooling, None)
    }

    /// Evict a job runtime that is currently cooling.
    pub fn evict_if_idle(&self, job_id: &str) -> Option<ThreadId> {
        let thread_id = self.get_thread_binding(job_id)?;
        self.evict_runtime(&thread_id, ThreadPoolEventReason::CoolingExpired)
            .map(|runtime| runtime.thread_id)
    }

    /// Evict a chat runtime that is currently cooling.
    pub fn evict_chat_if_idle(&self, thread_id: &ThreadId) -> Option<ThreadPoolRuntimeRef> {
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
        let sender = {
            let mut store = self.store.lock().expect("thread-pool mutex poisoned");
            let sender = {
                let entry = store
                    .runtimes
                    .get_mut(&thread_id.to_string())
                    .ok_or_else(|| {
                        JobError::ExecutionFailed(format!("thread {} is not registered", thread_id))
                    })?;
                entry.summary.status = ThreadRuntimeStatus::Running;
                entry.summary.estimated_memory_bytes = estimated_memory_bytes;
                entry.summary.last_active_at = Some(Utc::now().to_rfc3339());
                entry.summary.last_reason = None;
                entry.sender.clone()
            };
            Self::refresh_peaks(&mut store);
            sender
        };

        let _ = sender.send(ThreadEvent::ThreadPoolStarted {
            runtime: ThreadPoolRuntimeRef {
                thread_id,
                kind: ThreadPoolRuntimeKind::Chat,
                session_id: Some(session_id),
                job_id: None,
            },
        });
        let _ = sender.send(ThreadEvent::ThreadPoolMetricsUpdated {
            snapshot: self.collect_metrics(),
        });

        let mailbox = {
            let guard = thread.read().await;
            guard.mailbox()
        };
        mailbox.lock().await.enqueue_user_message(message, None);
        {
            let guard = thread.read().await;
            let _ = guard.control_tx().send(ThreadControlEvent::MailboxUpdated);
        }

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
            ThreadPoolRuntimeRef {
                thread_id,
                kind: ThreadPoolRuntimeKind::Chat,
                session_id: Some(session_id),
                job_id: None,
            },
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
        self.attach_chat_runtime(thread_id, session_id, Arc::clone(&thread), runtime_rx)
            .await?;
        argus_agent::Thread::spawn_reactor(Arc::clone(&thread));
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
        {
            let mut store = self.store.lock().expect("thread-pool mutex poisoned");
            if let Some(entry) = store.runtimes.get_mut(&execution_thread_id.to_string()) {
                entry.summary.status = ThreadRuntimeStatus::Running;
                entry.summary.estimated_memory_bytes = estimated_memory_bytes;
                entry.summary.last_active_at = Some(started_at.clone());
                entry.summary.last_reason = None;
            }
            Self::refresh_peaks(&mut store);
        }
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
            runtime: ThreadPoolRuntimeRef {
                thread_id: execution_thread_id,
                kind: ThreadPoolRuntimeKind::Job,
                session_id: None,
                job_id: Some(request.job_id.clone()),
            },
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
                            let mailbox = {
                                let guard = cancellation_thread.read().await;
                                guard.mailbox()
                            };
                            mailbox.lock().await.interrupt_stop();
                            let guard = cancellation_thread.read().await;
                            let _ = guard.control_tx().send(ThreadControlEvent::MailboxUpdated);
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
            if self.admission_waiters.load(Ordering::SeqCst) > 0
                && let Some((runtime, snapshot, shutdown)) = Self::evict_runtime_from_shared_store(
                    &self.store,
                    self.max_threads,
                    &execution_thread_id,
                    ThreadPoolEventReason::MemoryPressure,
                )
            {
                shutdown.run();
                let _ = pipe_tx.send(ThreadEvent::ThreadPoolEvicted {
                    runtime,
                    reason: ThreadPoolEventReason::MemoryPressure,
                });
                let _ = pipe_tx.send(ThreadEvent::ThreadPoolMetricsUpdated { snapshot });
            } else {
                let _ = pipe_tx.send(ThreadEvent::ThreadPoolCooling { runtime });
                let _ = pipe_tx.send(ThreadEvent::ThreadPoolMetricsUpdated { snapshot });
            }
        }

        result
    }

    fn update_state(
        &self,
        thread_id: &ThreadId,
        state: ThreadRuntimeStatus,
        reason: Option<ThreadPoolEventReason>,
    ) -> Option<ThreadId> {
        let mut store = self.store.lock().expect("thread-pool mutex poisoned");
        let runtime_thread_id = {
            let entry = store.runtimes.get_mut(&thread_id.to_string())?;
            entry.summary.status = state;
            entry.summary.last_active_at = Some(Utc::now().to_rfc3339());
            entry.summary.last_reason = reason;
            entry.summary.runtime.thread_id
        };
        Self::refresh_peaks(&mut store);
        Some(runtime_thread_id)
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
        runtime: ThreadPoolRuntimeRef,
        status: ThreadRuntimeStatus,
        estimated_memory_bytes: u64,
        last_active_at: Option<String>,
        recoverable: bool,
        last_reason: Option<ThreadPoolEventReason>,
        thread: Option<Arc<RwLock<argus_agent::Thread>>>,
    ) -> broadcast::Sender<ThreadEvent> {
        let mut store = self.store.lock().expect("thread-pool mutex poisoned");
        let runtime_key = runtime.thread_id.to_string();
        let (
            sender,
            existing_thread,
            existing_control_tx,
            existing_forwarder_abort,
            existing_slot_permit,
            load_mutex,
        ) = if let Some(entry) = store.runtimes.get_mut(&runtime_key) {
            (
                entry.sender.clone(),
                entry.thread.clone(),
                entry.control_tx.clone(),
                entry.forwarder_abort.take(),
                entry.slot_permit.take(),
                Arc::clone(&entry.load_mutex),
            )
        } else {
            let (sender, _rx) = broadcast::channel(256);
            (
                sender,
                None,
                None,
                None,
                None,
                Arc::new(AsyncMutex::new(())),
            )
        };
        store.runtimes.insert(
            runtime_key,
            RuntimeEntry {
                summary: ThreadPoolRuntimeSummary {
                    runtime,
                    status,
                    estimated_memory_bytes,
                    last_active_at,
                    recoverable,
                    last_reason,
                },
                sender: sender.clone(),
                thread: thread.or(existing_thread),
                control_tx: existing_control_tx,
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
            control_tx: entry.control_tx.take(),
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
        ThreadPoolRuntimeRef,
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
        let runtime = entry.summary.runtime.clone();
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
    ) -> Option<ThreadPoolRuntimeRef> {
        let candidate = {
            let store = self.store.lock().expect("thread-pool mutex poisoned");
            store
                .runtimes
                .values()
                .filter(|entry| entry.summary.status == ThreadRuntimeStatus::Cooling)
                .min_by_key(|entry| entry.summary.last_active_at.clone())
                .map(|entry| entry.summary.runtime.thread_id)
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
        let control_tx = {
            let guard = thread.read().await;
            guard.control_tx()
        };
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
                entry.control_tx = Some(control_tx.clone());
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
                                let runtime = entry.summary.runtime.clone();
                                ThreadPool::refresh_peaks(&mut store);
                                let snapshot =
                                    ThreadPool::collect_metrics_from_store(max_threads, &store);
                                (runtime, snapshot)
                            };

                            if admission_waiters.load(Ordering::SeqCst) > 0 {
                                if let Some((runtime, snapshot, shutdown)) =
                                    ThreadPool::evict_runtime_from_shared_store(
                                        &store,
                                        max_threads,
                                        &thread_id,
                                        ThreadPoolEventReason::MemoryPressure,
                                    )
                                {
                                    shutdown.run();
                                    let _ = sender.send(ThreadEvent::ThreadPoolEvicted {
                                        runtime,
                                        reason: ThreadPoolEventReason::MemoryPressure,
                                    });
                                    let _ = sender
                                        .send(ThreadEvent::ThreadPoolMetricsUpdated { snapshot });
                                }
                            } else {
                                let _ = sender.send(ThreadEvent::ThreadPoolCooling { runtime });
                                let _ =
                                    sender.send(ThreadEvent::ThreadPoolMetricsUpdated { snapshot });
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
        argus_agent::Thread::spawn_reactor(Arc::clone(&thread));
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
    ) -> Option<ThreadPoolRuntimeRef> {
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
            runtime: runtime.clone(),
            reason,
        });
        let _ = sender.send(ThreadEvent::ThreadPoolMetricsUpdated { snapshot });
        Some(runtime)
    }

    fn evict_runtime_from_shared_store(
        store: &Arc<StdMutex<ThreadPoolStore>>,
        max_threads: u32,
        thread_id: &ThreadId,
        reason: ThreadPoolEventReason,
    ) -> Option<(ThreadPoolRuntimeRef, ThreadPoolSnapshot, RuntimeShutdown)> {
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
        let runtime = entry.summary.runtime.clone();
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
        let base_dir = thread_base_dir(
            &self.chat_runtime_config.trace_dir,
            Some(session_id),
            thread_id,
        );
        let agent_record = recover_thread_snapshot(&base_dir)
            .await
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        let provider_id = ProviderId::new(thread_record.provider_id.into_inner());
        let requested_model = thread_record
            .model_override
            .clone()
            .unwrap_or_else(|| format!("provider-{}", provider_id.inner()));
        let provider = match thread_record.model_override.as_deref() {
            Some(model) => match self
                .provider_resolver
                .resolve_with_model(provider_id, model)
                .await
            {
                Ok(provider) => Ok(provider),
                Err(_) => self.provider_resolver.resolve(provider_id).await,
            },
            None => self.provider_resolver.resolve(provider_id).await,
        };
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

        let trace_cfg = TraceConfig::new(true, self.chat_runtime_config.trace_dir.clone())
            .with_session_id(session_id)
            .with_model(Some(provider.model_name().to_string()));
        let mut turn_config = TurnConfig::new();
        turn_config.trace_config = Some(trace_cfg);
        turn_config.on_turn_complete = Some(Self::build_on_turn_complete(
            self.chat_runtime_config.trace_dir.clone(),
        ));
        let config = ThreadConfigBuilder::default()
            .turn_config(turn_config)
            .build()
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        let thread_builder = ThreadBuilder::new()
            .id(thread_id)
            .session_id(session_id)
            .agent_record(Arc::new(agent_record))
            .title(thread_record.title.clone())
            .provider(provider.clone())
            .tool_manager(self.tool_manager.clone())
            .compactor(Arc::new(LlmCompactor::new(provider)));
        let plan_store = FilePlanStore::new(
            self.chat_runtime_config.trace_dir.clone(),
            &thread_id.inner().to_string(),
        );
        let thread = thread_builder
            .plan_store(plan_store)
            .config(config)
            .build()
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        let thread = Arc::new(RwLock::new(thread));

        let updated_at = chrono::DateTime::parse_from_rfc3339(&thread_record.updated_at)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now());
        let recovered = recover_thread_log_state(&base_dir)
            .await
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        if recovered.turn_count() > 0 {
            thread
                .write()
                .await
                .hydrate_from_turn_log_state(recovered, updated_at);
        }

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
        let base_dir = thread_base_dir(&self.chat_runtime_config.trace_dir, None, thread_id);
        let agent_record = if thread_record.is_some() {
            recover_thread_snapshot(&base_dir)
                .await
                .map_err(|err| JobError::ExecutionFailed(err.to_string()))?
        } else {
            let template_id = RepoAgentId::new(request.agent_id.inner());
            self.template_manager
                .get(AgentId::new(template_id.inner()))
                .await
                .map_err(|err| {
                    JobError::ExecutionFailed(format!(
                        "failed to load agent {}: {err}",
                        template_id.inner()
                    ))
                })?
                .ok_or_else(|| {
                    JobError::ExecutionFailed(format!("agent {} not found", template_id.inner()))
                })?
        };
        let provider = if let Some(thread_record) = thread_record.as_ref() {
            let provider_id = ProviderId::new(thread_record.provider_id.into_inner());
            match thread_record.model_override.as_deref() {
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
        } else if let Some(provider_id) = agent_record.provider_id {
            match agent_record.model_id.as_deref() {
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
        } else {
            self.provider_resolver.default_provider().await
        }
        .map_err(|err| JobError::ExecutionFailed(format!("failed to resolve provider: {err}")))?;

        let trace_cfg = TraceConfig::new(true, self.chat_runtime_config.trace_dir.clone())
            .with_model(Some(provider.model_name().to_string()));
        let mut turn_config = TurnConfig::new();
        turn_config.trace_config = Some(trace_cfg);
        let config = ThreadConfigBuilder::default()
            .turn_config(turn_config)
            .build()
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        let plan_store = FilePlanStore::new(
            self.chat_runtime_config.trace_dir.clone(),
            &thread_id.to_string(),
        );
        let thread_title = thread_record
            .as_ref()
            .and_then(|record| record.title.clone())
            .or_else(|| Some(format!("job:{}", request.job_id)));
        let thread = ThreadBuilder::new()
            .id(thread_id)
            .session_id(Self::job_runtime_session_id(thread_id))
            .agent_record(Arc::new(agent_record))
            .title(thread_title)
            .provider(provider.clone())
            .tool_manager(self.tool_manager.clone())
            .compactor(Arc::new(LlmCompactor::new(provider)))
            .plan_store(plan_store)
            .config(config)
            .build()
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        let thread = Arc::new(RwLock::new(thread));

        if let Some(thread_record) = thread_record {
            let updated_at = chrono::DateTime::parse_from_rfc3339(&thread_record.updated_at)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now());
            let recovered = recover_thread_log_state(&base_dir)
                .await
                .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
            if recovered.turn_count() > 0 {
                thread
                    .write()
                    .await
                    .hydrate_from_turn_log_state(recovered, updated_at);
            }
        }

        Ok(thread)
    }

    fn build_on_turn_complete(trace_dir: PathBuf) -> OnTurnComplete {
        Arc::new(move |sid: SessionId, turn_num: u32| {
            let trace_dir = trace_dir.clone();
            tokio::spawn(async move {
                let _ = ThreadPool::update_session_turn_file(&trace_dir, sid, turn_num).await;
            });
        })
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
        let Some(persistence) = &self.persistence else {
            return Ok(ThreadId::new());
        };

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
        let thread_id = existing_thread_id.unwrap_or_else(ThreadId::new);
        let trace_dir = thread_base_dir(&self.chat_runtime_config.trace_dir, None, thread_id);
        persist_thread_snapshot(&trace_dir, &agent_record)
            .await
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;

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
        persistence
            .thread_repository
            .upsert_thread(&thread_record)
            .await
            .map_err(|err| {
                let trace_dir = trace_dir.clone();
                tokio::spawn(async move {
                    let _ = tokio::fs::remove_dir_all(trace_dir).await;
                });
                JobError::ExecutionFailed(format!("failed to persist thread record: {err}"))
            })?;

        if existing_job.is_some() {
            if existing_thread_id.is_some() {
                return Ok(thread_id);
            }

            if let Err(err) =
                Self::persist_existing_job_binding(persistence, &job_id, thread_id).await
            {
                let _ = tokio::fs::remove_dir_all(&trace_dir).await;
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
            let _ = tokio::fs::remove_dir_all(&trace_dir).await;
            return Err(Self::rollback_thread_record(
                persistence,
                thread_id,
                format!("failed to create job record: {err}"),
            )
            .await);
        }

        Ok(thread_id)
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

    async fn update_session_turn_file(
        trace_dir: &Path,
        session_id: SessionId,
        turn_number: u32,
    ) -> std::io::Result<()> {
        let meta_path = trace_dir.join(session_id.to_string()).join("meta.json");
        let meta = if meta_path.exists() {
            let content = tokio::fs::read_to_string(&meta_path).await?;
            serde_json::from_str::<serde_json::Value>(&content).unwrap_or_else(|_| {
                serde_json::json!({
                    "session_id": session_id.to_string(),
                    "current_turn": 0,
                })
            })
        } else {
            serde_json::json!({
                "session_id": session_id.to_string(),
                "current_turn": 0,
            })
        };

        let updated = serde_json::json!({
            "session_id": meta.get("session_id")
                .and_then(|value| value.as_str())
                .unwrap_or(&session_id.to_string()),
            "current_turn": turn_number,
        });
        tokio::fs::write(&meta_path, serde_json::to_string_pretty(&updated)?).await
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
    use argus_protocol::llm::{CompletionRequest, CompletionResponse, LlmError, LlmEventStream};
    use argus_protocol::{AgentRecord, AgentType, ProviderId, ThinkingConfig};
    use argus_repository::ArgusSqlite;
    use argus_repository::migrate;
    use argus_repository::traits::{
        AgentRepository, JobRepository, LlmProviderRepository, ThreadRepository,
    };
    use argus_template::TemplateManager;
    use async_trait::async_trait;
    use rust_decimal::Decimal;
    use sqlx::SqlitePool;

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
        template_manager
            .upsert(AgentRecord {
                id: agent_id,
                display_name: "Job Snapshot Agent".to_string(),
                description: "Original job template".to_string(),
                version: "1.0.0".to_string(),
                provider_id: Some(ProviderId::new(1)),
                model_id: Some("job-test".to_string()),
                system_prompt: "You are the original job snapshot agent.".to_string(),
                tool_names: vec![],
                max_tokens: None,
                temperature: None,
                thinking_config: Some(ThinkingConfig::disabled()),
                parent_agent_id: None,
                agent_type: AgentType::Standard,
            })
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

        let request = test_request("job-snapshot");
        let thread_id = thread_pool
            .persist_binding(&request, &Utc::now().to_string())
            .await
            .expect("job binding should persist");

        let snapshot_path = trace_dir.join(thread_id.to_string()).join("thread.json");
        let snapshot_content = tokio::fs::read_to_string(&snapshot_path)
            .await
            .expect("job thread snapshot should exist");
        let snapshot: AgentRecord =
            serde_json::from_str(&snapshot_content).expect("job snapshot should deserialize");
        assert_eq!(
            snapshot.system_prompt,
            "You are the original job snapshot agent."
        );

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
                max_tokens: None,
                temperature: None,
                thinking_config: Some(ThinkingConfig::disabled()),
                parent_agent_id: None,
                agent_type: AgentType::Standard,
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
    }
}
