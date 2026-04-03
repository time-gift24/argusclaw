//! ThreadPool for coordinating unified job and chat runtimes.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use argus_agent::config::ThreadConfigBuilder;
use argus_agent::turn_log_store::recover_thread_log_state;
use argus_agent::{
    Compactor, FilePlanStore, OnTurnComplete, ThreadBuilder, TraceConfig, TurnCancellation,
    TurnConfig,
};
use argus_protocol::llm::{
    ChatMessage, CompletionRequest, CompletionResponse, LlmError, LlmEventStream, Role,
};
use argus_protocol::{
    AgentId, MailboxMessage, MailboxMessageType, ProviderId, ProviderResolver, SessionId,
    ThreadControlEvent, ThreadEvent, ThreadId, ThreadJobResult, ThreadPoolEventReason,
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

#[cfg(test)]
use argus_agent::turn_log_store::RecoveredThreadLogState;
#[cfg(test)]
use std::io;

use crate::error::JobError;
use crate::types::ThreadPoolJobRequest;

const DEFAULT_MAX_THREADS: u32 = 8;

#[derive(Clone)]
struct ChatRuntimeConfig {
    default_compactor: Arc<dyn Compactor>,
    trace_dir: PathBuf,
}

impl std::fmt::Debug for ChatRuntimeConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ChatRuntimeConfig")
            .field("trace_dir", &self.trace_dir)
            .finish()
    }
}

#[cfg(test)]
#[derive(Debug)]
struct RecoveredThreadState {
    messages: Vec<ChatMessage>,
    turn_count: u32,
    token_count: u32,
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
        default_compactor: Arc<dyn Compactor>,
        trace_dir: PathBuf,
    ) -> Self {
        Self::with_persistence(
            template_manager,
            provider_resolver,
            tool_manager,
            default_compactor,
            trace_dir,
            None,
        )
    }

    /// Create a thread pool with optional repository-backed persistence.
    pub fn with_persistence(
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        tool_manager: Arc<ToolManager>,
        default_compactor: Arc<dyn Compactor>,
        trace_dir: PathBuf,
        persistence: Option<ThreadPoolPersistence>,
    ) -> Self {
        Self {
            template_manager,
            provider_resolver,
            tool_manager,
            chat_runtime_config: ChatRuntimeConfig {
                default_compactor,
                trace_dir,
            },
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

        {
            let guard = thread.read().await;
            guard
                .send_control_event(ThreadControlEvent::DeliverMailboxMessage(message.clone()))
                .map_err(|error| JobError::ExecutionFailed(error.to_string()))?;
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

        thread
            .read()
            .await
            .send_user_message(message, None)
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))
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
        _control_tx: mpsc::UnboundedSender<ThreadControlEvent>,
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
                            let guard = cancellation_thread.read().await;
                            let _ = guard.send_control_event(ThreadControlEvent::UserInterrupt {
                                content: "cancel active job".to_string(),
                            });
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
                .history()
                .iter()
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
        let template_id = thread_record.template_id.ok_or_else(|| {
            JobError::ExecutionFailed(format!("thread {} is missing template_id", thread_id))
        })?;
        let agent_record = self
            .template_manager
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
            })?;
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
            .provider(provider)
            .tool_manager(self.tool_manager.clone())
            .compactor(self.chat_runtime_config.default_compactor.clone());
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
        let base_dir = self
            .chat_runtime_config
            .trace_dir
            .join(session_id.to_string())
            .join(thread_id.to_string());
        let recovered = recover_thread_log_state(
            &base_dir,
            (thread_record.turn_count > 0).then_some(thread_record.turn_count),
        )
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
        let template_id = thread_record
            .as_ref()
            .and_then(|record| record.template_id)
            .unwrap_or_else(|| RepoAgentId::new(request.agent_id.inner()));
        let agent_record = self
            .template_manager
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
            })?;
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
            .provider(provider)
            .tool_manager(self.tool_manager.clone())
            .compactor(self.chat_runtime_config.default_compactor.clone())
            .plan_store(plan_store)
            .config(config)
            .build()
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        let thread = Arc::new(RwLock::new(thread));

        if let Some(thread_record) = thread_record {
            let updated_at = chrono::DateTime::parse_from_rfc3339(&thread_record.updated_at)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now());
            let base_dir = self
                .chat_runtime_config
                .trace_dir
                .join(thread_id.to_string());
            let recovered = recover_thread_log_state(
                &base_dir,
                (thread_record.turn_count > 0).then_some(thread_record.turn_count),
            )
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
            .history()
            .iter()
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
        let model_override = agent_record.model_id;
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
                JobError::ExecutionFailed(format!("failed to persist thread record: {err}"))
            })?;

        if existing_job.is_some() {
            if existing_thread_id.is_some() {
                return Ok(thread_id);
            }

            if let Err(err) =
                Self::persist_existing_job_binding(persistence, &job_id, thread_id).await
            {
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
    async fn recover_thread_state_from_trace(
        trace_dir: &Path,
        session_id: &SessionId,
        thread_id: &ThreadId,
        turn_count_hint: Option<u32>,
    ) -> Result<RecoveredThreadState, io::Error> {
        let base_dir = trace_dir
            .join(session_id.to_string())
            .join(thread_id.to_string());
        let recovered = recover_thread_log_state(&base_dir, turn_count_hint)
            .await
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
        Ok(Self::flatten_recovered_thread_state(recovered))
    }

    #[cfg(test)]
    async fn recover_job_thread_state_from_trace(
        trace_dir: &Path,
        thread_id: &ThreadId,
        turn_count_hint: Option<u32>,
    ) -> Result<RecoveredThreadState, io::Error> {
        let base_dir = trace_dir.join(thread_id.to_string());
        let recovered = recover_thread_log_state(&base_dir, turn_count_hint)
            .await
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error.to_string()))?;
        Ok(Self::flatten_recovered_thread_state(recovered))
    }

    #[cfg(test)]
    fn flatten_recovered_thread_state(recovered: RecoveredThreadLogState) -> RecoveredThreadState {
        RecoveredThreadState {
            messages: recovered.committed_messages(),
            turn_count: recovered.turn_count(),
            token_count: recovered.token_count(),
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
            noop_compactor(),
            std::env::temp_dir().join("argus-thread-pool-tests"),
        )
    }
}

#[cfg(test)]
pub(crate) fn noop_compactor() -> Arc<dyn Compactor> {
    #[derive(Debug)]
    struct NoopCompactor;

    #[async_trait::async_trait]
    impl Compactor for NoopCompactor {
        async fn compact(
            &self,
            _provider: &dyn argus_protocol::LlmProvider,
            _messages: &[argus_protocol::llm::ChatMessage],
            _token_count: u32,
        ) -> std::result::Result<Option<argus_agent::CompactResult>, argus_agent::CompactError>
        {
            Ok(None)
        }

        fn name(&self) -> &'static str {
            "noop"
        }
    }

    Arc::new(NoopCompactor)
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
    use std::collections::HashMap;
    use std::collections::HashSet;
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::Mutex as StdMutex;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::Duration;

    use argus_agent::TurnCancellation;
    use argus_agent::history::TurnState;
    use argus_agent::turn_log_store::{
        TurnLogMeta, turn_messages_path, turn_meta_path, turns_dir, write_turn_messages,
        write_turn_meta,
    };
    use argus_protocol::llm::{
        ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LlmError, LlmProviderId,
        LlmProviderRepository,
    };
    use argus_protocol::{
        AgentId, AgentRecord, AgentType, LlmProvider, LlmProviderKind, LlmProviderRecord,
        ProviderId, ProviderResolver, ProviderSecretStatus, SecretString, SessionId,
        ThinkingConfig, ThreadEvent, ThreadId, ThreadPoolEventReason, ThreadRuntimeStatus,
        TokenUsage,
    };
    use argus_repository::traits::{
        AgentRepository, JobRepository, SessionRepository, ThreadRepository,
    };
    use argus_repository::types::{
        AgentId as RepoAgentId, JobId, JobRecord, JobResult, JobStatus, JobType, MessageId,
        MessageRecord, ThreadRecord,
    };
    use argus_repository::{ArgusSqlite, DbError, migrate};
    use argus_template::TemplateManager;
    use argus_tool::ToolManager;
    use async_trait::async_trait;
    use chrono::Utc;
    use rust_decimal::Decimal;
    use sqlx::SqlitePool;
    use tokio::sync::{RwLock, Semaphore, broadcast, mpsc};
    use tokio::task::yield_now;
    use tokio::time::{sleep, timeout};

    use super::{ThreadPool, ThreadPoolPersistence};

    #[derive(Debug)]
    struct DummyProviderResolver;

    #[async_trait]
    impl ProviderResolver for DummyProviderResolver {
        async fn resolve(&self, _id: ProviderId) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
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

    struct FixedProviderResolver {
        provider: Arc<dyn LlmProvider>,
    }

    impl FixedProviderResolver {
        fn new(provider: Arc<dyn LlmProvider>) -> Self {
            Self { provider }
        }
    }

    struct CountingDelayedProviderResolver {
        provider: Arc<dyn LlmProvider>,
        delay: Duration,
        resolve_calls: Arc<AtomicUsize>,
    }

    impl CountingDelayedProviderResolver {
        fn new(
            provider: Arc<dyn LlmProvider>,
            delay: Duration,
            resolve_calls: Arc<AtomicUsize>,
        ) -> Self {
            Self {
                provider,
                delay,
                resolve_calls,
            }
        }

        async fn resolve_provider(&self) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
            self.resolve_calls.fetch_add(1, Ordering::SeqCst);
            sleep(self.delay).await;
            Ok(Arc::clone(&self.provider))
        }
    }

    #[async_trait]
    impl ProviderResolver for CountingDelayedProviderResolver {
        async fn resolve(&self, _id: ProviderId) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
            self.resolve_provider().await
        }

        async fn default_provider(&self) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
            self.resolve_provider().await
        }

        async fn resolve_with_model(
            &self,
            _id: ProviderId,
            _model: &str,
        ) -> argus_protocol::Result<Arc<dyn LlmProvider>> {
            self.resolve_provider().await
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

    fn drain_events(rx: &mut broadcast::Receiver<ThreadEvent>) -> Vec<ThreadEvent> {
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        events
    }

    async fn wait_for_thread_event<F>(
        rx: &mut broadcast::Receiver<ThreadEvent>,
        predicate: F,
    ) -> ThreadEvent
    where
        F: Fn(&ThreadEvent) -> bool,
    {
        timeout(Duration::from_secs(5), async {
            loop {
                match rx.recv().await {
                    Ok(event) if predicate(&event) => break event,
                    Ok(_) => {}
                    Err(broadcast::error::RecvError::Closed) => {
                        panic!("thread event channel should stay open")
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                }
            }
        })
        .await
        .expect("matching thread event should arrive")
    }

    async fn test_persistent_pool() -> (ThreadPool, Arc<ArgusSqlite>) {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("sqlite memory pool should connect");
        migrate(&pool).await.expect("migration should succeed");
        let sqlite = Arc::new(ArgusSqlite::new(pool));
        let thread_pool = ThreadPool::with_persistence(
            Arc::new(TemplateManager::new(
                sqlite.clone() as Arc<dyn AgentRepository>,
                sqlite.clone(),
            )),
            Arc::new(DummyProviderResolver),
            Arc::new(ToolManager::new()),
            super::noop_compactor(),
            std::env::temp_dir().join("argus-thread-pool-tests"),
            Some(ThreadPoolPersistence::new(
                sqlite.clone() as Arc<dyn JobRepository>,
                sqlite.clone() as Arc<dyn ThreadRepository>,
                sqlite.clone() as Arc<dyn LlmProviderRepository>,
            )),
        );

        (thread_pool, sqlite)
    }

    async fn test_recoverable_persistent_pool() -> (ThreadPool, Arc<ArgusSqlite>) {
        let (thread_pool, sqlite) = test_persistent_pool().await;
        let provider_id =
            LlmProviderRepository::upsert_provider(sqlite.as_ref(), &sample_provider_record(true))
                .await
                .expect("provider upsert should succeed");

        thread_pool
            .template_manager
            .upsert(AgentRecord {
                id: argus_protocol::AgentId::new(7),
                display_name: "Recoverable ThreadPool Agent".to_string(),
                description: "Used to test thread recovery".to_string(),
                version: "1.0.0".to_string(),
                provider_id: Some(ProviderId::new(provider_id.into_inner())),
                model_id: Some("gpt-4o-mini".to_string()),
                system_prompt: "You are a test agent.".to_string(),
                tool_names: vec![],
                max_tokens: None,
                temperature: None,
                thinking_config: Some(ThinkingConfig::enabled()),
                parent_agent_id: None,
                agent_type: AgentType::Standard,
            })
            .await
            .expect("agent upsert should succeed");

        (thread_pool, sqlite)
    }

    async fn test_runtime_pool_with_resolver(
        max_threads: u32,
        provider_resolver: Arc<dyn ProviderResolver>,
    ) -> (ThreadPool, Arc<ArgusSqlite>, LlmProviderId) {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("sqlite memory pool should connect");
        migrate(&pool).await.expect("migration should succeed");
        let sqlite = Arc::new(ArgusSqlite::new(pool));

        let provider_id =
            LlmProviderRepository::upsert_provider(sqlite.as_ref(), &sample_provider_record(true))
                .await
                .expect("provider upsert should succeed");

        let template_manager = Arc::new(TemplateManager::new(
            sqlite.clone() as Arc<dyn AgentRepository>,
            sqlite.clone(),
        ));
        template_manager
            .upsert(AgentRecord {
                id: argus_protocol::AgentId::new(7),
                display_name: "Runtime Pool Test Agent".to_string(),
                description: "Used to test unified runtime behavior".to_string(),
                version: "1.0.0".to_string(),
                provider_id: Some(ProviderId::new(provider_id.into_inner())),
                model_id: Some("capturing".to_string()),
                system_prompt: "You are a runtime pool test agent.".to_string(),
                tool_names: vec![],
                max_tokens: None,
                temperature: None,
                thinking_config: Some(ThinkingConfig::enabled()),
                parent_agent_id: None,
                agent_type: AgentType::Standard,
            })
            .await
            .expect("agent upsert should succeed");

        let trace_dir =
            std::env::temp_dir().join(format!("argus-thread-pool-tests-{}", ThreadId::new()));
        std::fs::create_dir_all(&trace_dir).expect("trace dir should exist");

        let mut thread_pool = ThreadPool::with_persistence(
            template_manager,
            provider_resolver,
            Arc::new(ToolManager::new()),
            super::noop_compactor(),
            trace_dir,
            Some(ThreadPoolPersistence::new(
                sqlite.clone() as Arc<dyn JobRepository>,
                sqlite.clone() as Arc<dyn ThreadRepository>,
                sqlite.clone() as Arc<dyn LlmProviderRepository>,
            )),
        );
        thread_pool.max_threads = max_threads;
        thread_pool.resident_slots = Arc::new(Semaphore::new(max_threads as usize));

        (thread_pool, sqlite, provider_id)
    }

    async fn test_runtime_pool_with_limit(
        max_threads: u32,
        provider: Arc<dyn LlmProvider>,
    ) -> (ThreadPool, Arc<ArgusSqlite>, LlmProviderId) {
        test_runtime_pool_with_resolver(max_threads, Arc::new(FixedProviderResolver::new(provider)))
            .await
    }

    async fn seed_chat_thread(
        pool: &ThreadPool,
        sqlite: &Arc<ArgusSqlite>,
        provider_id: LlmProviderId,
        session_name: &str,
    ) -> (SessionId, ThreadId) {
        let session_id = SessionId::new();
        SessionRepository::create(sqlite.as_ref(), &session_id, session_name)
            .await
            .expect("session should persist");
        std::fs::create_dir_all(
            pool.chat_runtime_config
                .trace_dir
                .join(session_id.to_string()),
        )
        .expect("session trace dir should exist");

        let thread_id = ThreadId::new();
        ThreadRepository::upsert_thread(
            sqlite.as_ref(),
            &ThreadRecord {
                id: thread_id,
                provider_id,
                title: Some(format!("thread:{session_name}")),
                token_count: 0,
                turn_count: 0,
                session_id: Some(session_id),
                template_id: Some(RepoAgentId::new(7)),
                model_override: Some("capturing".to_string()),
                created_at: "2026-03-30T00:00:00Z".to_string(),
                updated_at: "2026-03-30T00:00:00Z".to_string(),
            },
        )
        .await
        .expect("thread should persist");
        pool.register_chat_thread(session_id, thread_id);

        (session_id, thread_id)
    }

    fn unique_trace_dir(prefix: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!("{prefix}-{}", ThreadId::new()));
        std::fs::create_dir_all(&path).expect("trace dir should exist");
        path
    }

    async fn write_committed_turn(
        base_dir: &Path,
        turn_number: u32,
        user_content: &str,
        assistant_content: &str,
        total_tokens: u32,
    ) {
        let turn_dir = turns_dir(base_dir);
        write_turn_messages(
            &turn_messages_path(&turn_dir, turn_number),
            &[
                ChatMessage::user(user_content),
                ChatMessage::assistant(assistant_content),
            ],
        )
        .await
        .expect("messages should write");
        write_turn_meta(
            &turn_meta_path(&turn_dir, turn_number),
            &TurnLogMeta {
                turn_number,
                state: TurnState::Completed,
                token_usage: Some(TokenUsage {
                    input_tokens: total_tokens / 2,
                    output_tokens: total_tokens.saturating_sub(total_tokens / 2),
                    total_tokens,
                }),
                started_at: Utc::now(),
                finished_at: Some(Utc::now()),
                model: Some("test-model".to_string()),
                error: None,
            },
        )
        .await
        .expect("meta should write");
    }

    async fn wait_for_thread_drop<T>(weak: &std::sync::Weak<T>) {
        timeout(Duration::from_secs(5), async {
            loop {
                if weak.upgrade().is_none() {
                    break;
                }
                yield_now().await;
            }
        })
        .await
        .expect("thread should be dropped");
    }

    fn sample_provider_record(is_default: bool) -> LlmProviderRecord {
        LlmProviderRecord {
            id: LlmProviderId::new(0),
            display_name: "thread-pool-provider".to_string(),
            kind: LlmProviderKind::OpenAiCompatible,
            base_url: "http://localhost:11434/v1".to_string(),
            api_key: SecretString::new("test-key"),
            models: vec!["gpt-4o-mini".to_string()],
            model_config: HashMap::new(),
            default_model: "gpt-4o-mini".to_string(),
            is_default,
            extra_headers: HashMap::new(),
            secret_status: ProviderSecretStatus::Ready,
            meta_data: HashMap::new(),
        }
    }

    async fn test_pool_with_failing_job_create() -> (ThreadPool, Arc<RecordingThreadRepository>) {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("sqlite memory pool should connect");
        migrate(&pool).await.expect("migration should succeed");
        let sqlite = Arc::new(ArgusSqlite::new(pool));

        let provider_id =
            LlmProviderRepository::upsert_provider(sqlite.as_ref(), &sample_provider_record(true))
                .await
                .expect("provider upsert should succeed");

        let template_manager = Arc::new(TemplateManager::new(
            sqlite.clone() as Arc<dyn AgentRepository>,
            sqlite.clone(),
        ));
        template_manager
            .upsert(AgentRecord {
                id: argus_protocol::AgentId::new(7),
                display_name: "Failing Job Create Agent".to_string(),
                description: "Used to test rollback".to_string(),
                version: "1.0.0".to_string(),
                provider_id: Some(ProviderId::new(provider_id.into_inner())),
                model_id: Some("gpt-4o-mini".to_string()),
                system_prompt: "You are a test agent.".to_string(),
                tool_names: vec![],
                max_tokens: None,
                temperature: None,
                thinking_config: Some(ThinkingConfig::enabled()),
                parent_agent_id: None,
                agent_type: AgentType::Standard,
            })
            .await
            .expect("agent upsert should succeed");

        let thread_repository = Arc::new(RecordingThreadRepository::default());
        let thread_pool = ThreadPool::with_persistence(
            template_manager,
            Arc::new(DummyProviderResolver),
            Arc::new(ToolManager::new()),
            super::noop_compactor(),
            std::env::temp_dir().join("argus-thread-pool-tests"),
            Some(ThreadPoolPersistence::new(
                Arc::new(FailingCreateJobRepository) as Arc<dyn JobRepository>,
                thread_repository.clone() as Arc<dyn ThreadRepository>,
                sqlite as Arc<dyn LlmProviderRepository>,
            )),
        );

        (thread_pool, thread_repository)
    }

    async fn test_pool_with_failing_update_thread_id(
        existing_thread_id: Option<ThreadId>,
    ) -> (ThreadPool, Arc<RecordingThreadRepository>, Option<ThreadId>) {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("sqlite memory pool should connect");
        migrate(&pool).await.expect("migration should succeed");
        let sqlite = Arc::new(ArgusSqlite::new(pool));

        let provider_id =
            LlmProviderRepository::upsert_provider(sqlite.as_ref(), &sample_provider_record(true))
                .await
                .expect("provider upsert should succeed");

        let template_manager = Arc::new(TemplateManager::new(
            sqlite.clone() as Arc<dyn AgentRepository>,
            sqlite.clone(),
        ));
        template_manager
            .upsert(AgentRecord {
                id: argus_protocol::AgentId::new(7),
                display_name: "Existing Job Recovery Agent".to_string(),
                description: "Used to test update_thread_id rollback".to_string(),
                version: "1.0.0".to_string(),
                provider_id: Some(ProviderId::new(provider_id.into_inner())),
                model_id: Some("gpt-4o-mini".to_string()),
                system_prompt: "You are a test agent.".to_string(),
                tool_names: vec![],
                max_tokens: None,
                temperature: None,
                thinking_config: Some(ThinkingConfig::enabled()),
                parent_agent_id: None,
                agent_type: AgentType::Standard,
            })
            .await
            .expect("agent upsert should succeed");

        let thread_repository = Arc::new(RecordingThreadRepository::default());

        let job_id = JobId::new("job-update-thread-id-fails");
        let agent_id = RepoAgentId::new(7);
        let job_thread_id = existing_thread_id;

        if let Some(thread_id) = job_thread_id {
            thread_repository
                .threads
                .lock()
                .expect("thread store mutex poisoned")
                .insert(
                    thread_id.to_string(),
                    ThreadRecord {
                        id: thread_id,
                        provider_id,
                        title: Some("job:job-update-thread-id-fails".to_string()),
                        token_count: 0,
                        turn_count: 0,
                        session_id: None,
                        template_id: Some(RepoAgentId::new(7)),
                        model_override: Some("gpt-4o-mini".to_string()),
                        created_at: "2026-03-29T00:00:00Z".to_string(),
                        updated_at: "2026-03-29T00:00:00Z".to_string(),
                    },
                );
        }

        let thread_pool = ThreadPool::with_persistence(
            template_manager,
            Arc::new(DummyProviderResolver),
            Arc::new(ToolManager::new()),
            super::noop_compactor(),
            std::env::temp_dir().join("argus-thread-pool-tests"),
            Some(ThreadPoolPersistence::new(
                Arc::new(FailingUpdateThreadIdJobRepository::new(
                    job_id,
                    job_thread_id,
                    agent_id,
                )) as Arc<dyn JobRepository>,
                thread_repository.clone() as Arc<dyn ThreadRepository>,
                sqlite as Arc<dyn LlmProviderRepository>,
            )),
        );

        (thread_pool, thread_repository, job_thread_id)
    }

    #[derive(Default)]
    struct RecordingThreadRepository {
        threads: StdMutex<HashMap<String, ThreadRecord>>,
        deleted_threads: StdMutex<HashSet<String>>,
    }

    #[async_trait]
    impl ThreadRepository for RecordingThreadRepository {
        async fn upsert_thread(&self, thread: &ThreadRecord) -> Result<(), DbError> {
            self.threads
                .lock()
                .expect("thread store mutex poisoned")
                .insert(thread.id.to_string(), thread.clone());
            Ok(())
        }

        async fn get_thread(
            &self,
            id: &argus_protocol::ThreadId,
        ) -> Result<Option<ThreadRecord>, DbError> {
            Ok(self
                .threads
                .lock()
                .expect("thread store mutex poisoned")
                .get(&id.to_string())
                .cloned())
        }

        async fn list_threads(&self, _limit: u32) -> Result<Vec<ThreadRecord>, DbError> {
            Ok(self
                .threads
                .lock()
                .expect("thread store mutex poisoned")
                .values()
                .cloned()
                .collect())
        }

        async fn list_threads_in_session(
            &self,
            _session_id: &argus_protocol::SessionId,
        ) -> Result<Vec<ThreadRecord>, DbError> {
            unreachable!("list_threads_in_session should not be called")
        }

        async fn delete_thread(&self, id: &argus_protocol::ThreadId) -> Result<bool, DbError> {
            let thread_key = id.to_string();
            let removed = self
                .threads
                .lock()
                .expect("thread store mutex poisoned")
                .remove(&thread_key)
                .is_some();
            self.deleted_threads
                .lock()
                .expect("deleted thread mutex poisoned")
                .insert(thread_key);
            Ok(removed)
        }

        async fn delete_threads_in_session(
            &self,
            _session_id: &argus_protocol::SessionId,
        ) -> Result<u64, DbError> {
            unreachable!("delete_threads_in_session should not be called")
        }

        async fn add_message(&self, _message: &MessageRecord) -> Result<MessageId, DbError> {
            unreachable!("add_message should not be called")
        }

        async fn get_messages(
            &self,
            _thread_id: &argus_protocol::ThreadId,
        ) -> Result<Vec<MessageRecord>, DbError> {
            unreachable!("get_messages should not be called")
        }

        async fn get_recent_messages(
            &self,
            _thread_id: &argus_protocol::ThreadId,
            _limit: u32,
        ) -> Result<Vec<MessageRecord>, DbError> {
            unreachable!("get_recent_messages should not be called")
        }

        async fn delete_messages_before(
            &self,
            _thread_id: &argus_protocol::ThreadId,
            _seq: u32,
        ) -> Result<u64, DbError> {
            unreachable!("delete_messages_before should not be called")
        }

        async fn update_thread_stats(
            &self,
            _id: &argus_protocol::ThreadId,
            _token_count: u32,
            _turn_count: u32,
        ) -> Result<(), DbError> {
            unreachable!("update_thread_stats should not be called")
        }

        async fn rename_thread(
            &self,
            _id: &argus_protocol::ThreadId,
            _session_id: &argus_protocol::SessionId,
            _title: Option<&str>,
        ) -> Result<bool, DbError> {
            unreachable!("rename_thread should not be called")
        }

        async fn update_thread_model(
            &self,
            _id: &argus_protocol::ThreadId,
            _session_id: &argus_protocol::SessionId,
            _provider_id: LlmProviderId,
            _model_override: Option<&str>,
        ) -> Result<bool, DbError> {
            unreachable!("update_thread_model should not be called")
        }

        async fn get_thread_in_session(
            &self,
            _thread_id: &argus_protocol::ThreadId,
            _session_id: &argus_protocol::SessionId,
        ) -> Result<Option<ThreadRecord>, DbError> {
            unreachable!("get_thread_in_session should not be called")
        }
    }

    struct FailingCreateJobRepository;

    #[async_trait]
    impl JobRepository for FailingCreateJobRepository {
        async fn create(&self, _job: &JobRecord) -> Result<(), DbError> {
            Err(DbError::QueryFailed {
                reason: "simulated job create failure".to_string(),
            })
        }

        async fn get(&self, _id: &JobId) -> Result<Option<JobRecord>, DbError> {
            Ok(None)
        }

        async fn update_status(
            &self,
            _id: &JobId,
            _status: JobStatus,
            _started_at: Option<&str>,
            _finished_at: Option<&str>,
        ) -> Result<(), DbError> {
            Ok(())
        }

        async fn update_result(&self, _id: &JobId, _result: &JobResult) -> Result<(), DbError> {
            unreachable!("update_result should not be called")
        }

        async fn update_thread_id(
            &self,
            _id: &JobId,
            _thread_id: &argus_protocol::ThreadId,
        ) -> Result<(), DbError> {
            unreachable!("update_thread_id should not be called")
        }

        async fn find_ready_jobs(&self, _limit: usize) -> Result<Vec<JobRecord>, DbError> {
            unreachable!("find_ready_jobs should not be called")
        }

        async fn find_due_cron_jobs(&self, _now: &str) -> Result<Vec<JobRecord>, DbError> {
            unreachable!("find_due_cron_jobs should not be called")
        }

        async fn update_scheduled_at(&self, _id: &JobId, _next: &str) -> Result<(), DbError> {
            unreachable!("update_scheduled_at should not be called")
        }

        async fn list_by_group(&self, _group_id: &str) -> Result<Vec<JobRecord>, DbError> {
            unreachable!("list_by_group should not be called")
        }

        async fn delete(&self, _id: &JobId) -> Result<bool, DbError> {
            unreachable!("delete should not be called")
        }
    }

    struct FailingUpdateThreadIdJobRepository {
        job_id: JobId,
        thread_id: Option<ThreadId>,
        agent_id: RepoAgentId,
    }

    impl FailingUpdateThreadIdJobRepository {
        fn new(job_id: JobId, thread_id: Option<ThreadId>, agent_id: RepoAgentId) -> Self {
            Self {
                job_id,
                thread_id,
                agent_id,
            }
        }
    }

    #[async_trait]
    impl JobRepository for FailingUpdateThreadIdJobRepository {
        async fn create(&self, _job: &JobRecord) -> Result<(), DbError> {
            unreachable!("create should not be called")
        }

        async fn get(&self, _id: &JobId) -> Result<Option<JobRecord>, DbError> {
            Ok(Some(JobRecord {
                id: self.job_id.clone(),
                job_type: JobType::Standalone,
                name: format!("job:{}", self.job_id),
                status: JobStatus::Pending,
                agent_id: self.agent_id,
                context: None,
                prompt: "run test".to_string(),
                thread_id: self.thread_id,
                group_id: None,
                depends_on: Vec::new(),
                cron_expr: None,
                scheduled_at: None,
                started_at: None,
                finished_at: None,
                parent_job_id: None,
                result: None,
            }))
        }

        async fn update_status(
            &self,
            _id: &JobId,
            _status: JobStatus,
            _started_at: Option<&str>,
            _finished_at: Option<&str>,
        ) -> Result<(), DbError> {
            Ok(())
        }

        async fn update_result(&self, _id: &JobId, _result: &JobResult) -> Result<(), DbError> {
            unreachable!("update_result should not be called")
        }

        async fn update_thread_id(
            &self,
            _id: &JobId,
            _thread_id: &argus_protocol::ThreadId,
        ) -> Result<(), DbError> {
            Err(DbError::QueryFailed {
                reason: "simulated update_thread_id failure".to_string(),
            })
        }

        async fn find_ready_jobs(&self, _limit: usize) -> Result<Vec<JobRecord>, DbError> {
            unreachable!("find_ready_jobs should not be called")
        }

        async fn find_due_cron_jobs(&self, _now: &str) -> Result<Vec<JobRecord>, DbError> {
            unreachable!("find_due_cron_jobs should not be called")
        }

        async fn update_scheduled_at(&self, _id: &JobId, _next: &str) -> Result<(), DbError> {
            unreachable!("update_scheduled_at should not be called")
        }

        async fn list_by_group(&self, _group_id: &str) -> Result<Vec<JobRecord>, DbError> {
            unreachable!("list_by_group should not be called")
        }

        async fn delete(&self, _id: &JobId) -> Result<bool, DbError> {
            unreachable!("delete should not be called")
        }
    }

    #[tokio::test]
    async fn enqueue_job_creates_binding_and_updates_metrics() {
        super::assert_enqueue_job_creates_binding_and_updates_metrics().await;
    }

    #[tokio::test]
    async fn collect_metrics_tracks_running_and_cooling_states() {
        let pool = ThreadPool::test_pool();
        pool.enqueue_job(super::test_request("job-2"))
            .await
            .expect("enqueue should succeed");

        pool.mark_running("job-2");
        let running = pool.collect_metrics();
        assert_eq!(running.running_threads, 1);

        pool.mark_cooling("job-2");
        let cooling = pool.collect_metrics();
        assert_eq!(cooling.cooling_threads, 1);
    }

    #[tokio::test]
    async fn cooling_runtime_can_be_evicted() {
        let pool = ThreadPool::test_pool();
        let thread_id = pool
            .enqueue_job(super::test_request("job-3"))
            .await
            .expect("enqueue should succeed");

        pool.mark_cooling("job-3");
        let evicted = pool
            .evict_if_idle("job-3")
            .expect("cooling runtime should be evicted");
        assert_eq!(evicted, thread_id);

        let snapshot = pool.collect_metrics();
        assert_eq!(snapshot.active_threads, 0);
        assert_eq!(snapshot.evicted_threads, 1);
    }

    #[tokio::test]
    async fn collect_metrics_tracks_cooling_then_evicted_transition() {
        let pool = ThreadPool::test_pool();
        pool.enqueue_job(super::test_request("job-3-metrics"))
            .await
            .expect("enqueue should succeed");

        pool.mark_cooling("job-3-metrics");
        let cooling = pool.collect_metrics();
        assert_eq!(cooling.cooling_threads, 1);
        assert_eq!(cooling.evicted_threads, 0);

        let _ = pool.evict_if_idle("job-3-metrics");
        let evicted = pool.collect_metrics();
        assert_eq!(evicted.cooling_threads, 0);
        assert_eq!(evicted.evicted_threads, 1);
        assert_eq!(evicted.active_threads, 0);
    }

    #[tokio::test]
    async fn evicted_runtime_reuses_persisted_thread_id_on_reenqueue() {
        let (pool, _sqlite) = test_recoverable_persistent_pool().await;
        let request = super::test_request("job-recovery");

        let first_thread_id = pool
            .enqueue_job(request.clone())
            .await
            .expect("first enqueue should succeed");
        pool.mark_cooling("job-recovery");
        let evicted_thread_id = pool
            .evict_if_idle("job-recovery")
            .expect("cooling runtime should be evicted");
        assert_eq!(evicted_thread_id, first_thread_id);
        assert_eq!(pool.collect_metrics().evicted_threads, 1);

        let recovered_thread_id = pool
            .enqueue_job(request)
            .await
            .expect("re-enqueue should recover persisted thread");

        assert_eq!(
            recovered_thread_id, first_thread_id,
            "recovered runtime should reuse the persisted thread id"
        );
        assert_eq!(
            pool.get_thread_binding("job-recovery"),
            Some(first_thread_id)
        );

        let snapshot = pool.collect_metrics();
        assert_eq!(snapshot.active_threads, 0);
        assert_eq!(snapshot.queued_threads, 1);
        assert_eq!(snapshot.evicted_threads, 0);
    }

    #[tokio::test]
    async fn reenqueue_preserves_persisted_thread_stats() {
        let (pool, sqlite) = test_recoverable_persistent_pool().await;
        let request = super::test_request("job-recovery-stats");

        let thread_id = pool
            .enqueue_job(request.clone())
            .await
            .expect("first enqueue should succeed");
        ThreadRepository::update_thread_stats(sqlite.as_ref(), &thread_id, 512, 3)
            .await
            .expect("thread stats update should succeed");
        pool.mark_cooling("job-recovery-stats");
        pool.evict_if_idle("job-recovery-stats")
            .expect("cooling runtime should be evicted");

        pool.enqueue_job(request)
            .await
            .expect("re-enqueue should succeed");

        let persisted_thread = ThreadRepository::get_thread(sqlite.as_ref(), &thread_id)
            .await
            .expect("thread lookup should succeed")
            .expect("thread should remain persisted");
        assert_eq!(persisted_thread.token_count, 512);
        assert_eq!(persisted_thread.turn_count, 3);
    }

    #[tokio::test]
    async fn reenqueue_existing_thread_skips_update_thread_id_even_if_repository_would_fail() {
        let (pool, thread_repository, thread_id) =
            test_pool_with_failing_update_thread_id(Some(ThreadId::new())).await;
        let thread_id = thread_id.expect("existing thread should be seeded");

        let result = pool
            .enqueue_job(super::test_request("job-update-thread-id-fails"))
            .await;

        let recovered_thread_id = result.expect("already-bound recovery should succeed");
        assert_eq!(recovered_thread_id, thread_id);
        assert_eq!(
            thread_repository
                .deleted_threads
                .lock()
                .expect("deleted thread mutex poisoned")
                .len(),
            0,
            "short-circuited recovery should not delete the existing thread"
        );
        assert!(
            thread_repository
                .get_thread(&thread_id)
                .await
                .expect("thread lookup should succeed")
                .is_some(),
            "existing thread record should remain persisted"
        );
    }

    #[tokio::test]
    async fn reenqueue_update_thread_id_failure_rolls_back_new_thread_record() {
        let (pool, thread_repository, _) = test_pool_with_failing_update_thread_id(None).await;

        let result = pool
            .enqueue_job(super::test_request("job-update-thread-id-fails"))
            .await;

        let error = result.expect_err("enqueue should fail when update_thread_id fails");
        assert!(
            error
                .to_string()
                .contains("failed to persist job-thread binding"),
            "unexpected error: {error}"
        );
        assert!(
            thread_repository
                .threads
                .lock()
                .expect("thread store mutex poisoned")
                .is_empty(),
            "newly created thread record should be rolled back"
        );
        assert_eq!(
            thread_repository
                .deleted_threads
                .lock()
                .expect("deleted thread mutex poisoned")
                .len(),
            1,
            "newly created thread should be deleted on update_thread_id failure"
        );
    }

    #[tokio::test]
    async fn execute_job_emits_running_metrics_snapshot() {
        let provider = Arc::new(CapturingProvider::new(
            "running metrics reply",
            Duration::from_millis(25),
            24,
        ));
        let (pool, _sqlite, _provider_id) = test_runtime_pool_with_limit(1, provider).await;
        let request = super::test_request("job-running-metrics");
        let execution_thread_id = pool
            .enqueue_job(request.clone())
            .await
            .expect("enqueue should succeed");
        let (pipe_tx, mut pipe_rx) = broadcast::channel(32);
        let (control_tx, _control_rx) = mpsc::unbounded_channel();

        let _ = pool
            .execute_job(
                request,
                execution_thread_id,
                pipe_tx,
                control_tx,
                TurnCancellation::new(),
            )
            .await;
        let events = drain_events(&mut pipe_rx);

        assert!(events.iter().any(|event| {
            matches!(
                event,
                ThreadEvent::ThreadPoolMetricsUpdated { snapshot }
                    if snapshot.running_threads == 1
                        && snapshot.queued_threads == 0
                        && snapshot.cooling_threads == 0
            )
        }));
    }

    #[tokio::test]
    async fn execute_job_persists_queued_and_succeeded_states_with_result() {
        let provider = Arc::new(CapturingProvider::new(
            "persisted success reply",
            Duration::from_millis(25),
            48,
        ));
        let (pool, sqlite, _provider_id) = test_runtime_pool_with_limit(1, provider).await;
        let request = super::test_request("job-persist-success");
        let execution_thread_id = pool
            .enqueue_job(request.clone())
            .await
            .expect("enqueue should succeed");

        let queued_job = JobRepository::get(sqlite.as_ref(), &JobId::new("job-persist-success"))
            .await
            .expect("queued job lookup should succeed")
            .expect("queued job should persist");
        assert_eq!(queued_job.status, JobStatus::Queued);
        assert_eq!(queued_job.thread_id, Some(execution_thread_id));

        let (pipe_tx, _pipe_rx) = broadcast::channel(32);
        let (control_tx, _control_rx) = mpsc::unbounded_channel();
        let result = pool
            .execute_job(
                request,
                execution_thread_id,
                pipe_tx,
                control_tx,
                TurnCancellation::new(),
            )
            .await;

        assert!(result.success);
        let persisted_job = JobRepository::get(sqlite.as_ref(), &JobId::new("job-persist-success"))
            .await
            .expect("completed job lookup should succeed")
            .expect("completed job should persist");
        assert_eq!(persisted_job.status, JobStatus::Succeeded);
        assert!(persisted_job.started_at.is_some());
        assert!(persisted_job.finished_at.is_some());
        let persisted_result = persisted_job.result.expect("job result should persist");
        assert!(persisted_result.success);
        assert_eq!(persisted_result.message, result.message);

        let persisted_thread = ThreadRepository::get_thread(sqlite.as_ref(), &execution_thread_id)
            .await
            .expect("thread lookup should succeed")
            .expect("thread should persist");
        assert_eq!(persisted_thread.turn_count, 1);
        assert!(
            persisted_thread.token_count > 0,
            "job completion should persist authoritative thread stats"
        );
    }

    #[tokio::test]
    async fn execute_job_pre_cancelled_before_assignment_does_not_start_turn() {
        let provider = Arc::new(CapturingProvider::new(
            "should never run",
            Duration::from_millis(100),
            20,
        ));
        let (pool, sqlite, _provider_id) = test_runtime_pool_with_limit(1, provider).await;
        let request = super::test_request("job-pre-cancelled");
        let execution_thread_id = pool
            .enqueue_job(request.clone())
            .await
            .expect("enqueue should succeed");
        let (pipe_tx, _pipe_rx) = broadcast::channel(32);
        let (control_tx, _control_rx) = mpsc::unbounded_channel();
        let cancellation = TurnCancellation::new();
        cancellation.cancel();

        let result = pool
            .execute_job(
                request,
                execution_thread_id,
                pipe_tx,
                control_tx,
                cancellation,
            )
            .await;

        assert!(!result.success);
        assert!(
            result.message.to_lowercase().contains("cancel"),
            "unexpected result message: {}",
            result.message
        );

        let persisted_thread = ThreadRepository::get_thread(sqlite.as_ref(), &execution_thread_id)
            .await
            .expect("thread lookup should succeed")
            .expect("thread should persist");
        assert_eq!(persisted_thread.turn_count, 0);
        assert_eq!(persisted_thread.token_count, 0);
    }

    #[tokio::test]
    async fn execute_job_panic_cleans_up_to_cooling_and_emits_metrics() {
        let provider = Arc::new(CapturingProvider::new(
            "panic cleanup reply",
            Duration::from_millis(25),
            24,
        ));
        let (pool, sqlite, _provider_id) = test_runtime_pool_with_limit(1, provider).await;
        let mut request = super::test_request("job-panic-cleanup");
        request.prompt = "__panic_thread_pool_execute_turn__".to_string();
        let execution_thread_id = pool
            .enqueue_job(request.clone())
            .await
            .expect("enqueue should succeed");
        let (pipe_tx, mut pipe_rx) = broadcast::channel(32);
        let (control_tx, _control_rx) = mpsc::unbounded_channel();

        let result = pool
            .execute_job(
                request,
                execution_thread_id,
                pipe_tx,
                control_tx,
                TurnCancellation::new(),
            )
            .await;
        let events = drain_events(&mut pipe_rx);

        assert!(!result.success);
        let snapshot = pool.collect_metrics();
        assert_eq!(snapshot.running_threads, 0);
        assert_eq!(snapshot.cooling_threads, 1);
        assert!(events.iter().any(|event| {
            matches!(
                event,
                ThreadEvent::ThreadPoolMetricsUpdated { snapshot }
                    if snapshot.cooling_threads == 1 && snapshot.running_threads == 0
            )
        }));
        let persisted_job = JobRepository::get(sqlite.as_ref(), &JobId::new("job-panic-cleanup"))
            .await
            .expect("failed job lookup should succeed")
            .expect("failed job should persist");
        assert_eq!(persisted_job.status, JobStatus::Failed);
        let persisted_result = persisted_job.result.expect("failed result should persist");
        assert!(!persisted_result.success);
        assert!(persisted_result.message.contains("job executor panicked"));
    }

    #[tokio::test]
    async fn send_chat_message_persists_thread_stats_after_idle() {
        let provider = Arc::new(CapturingProvider::new(
            "assistant reply with persisted stats",
            Duration::from_millis(25),
            48,
        ));
        let (pool, sqlite, provider_id) = test_runtime_pool_with_limit(2, provider).await;
        let (session_id, thread_id) =
            seed_chat_thread(&pool, &sqlite, provider_id, "persist-thread-stats").await;
        let mut rx = pool.register_chat_thread(session_id, thread_id);

        pool.send_chat_message(session_id, thread_id, "persist stats".to_string())
            .await
            .expect("chat message should enqueue");
        let _ = wait_for_thread_event(&mut rx, |event| {
            matches!(
                event,
                ThreadEvent::ThreadPoolCooling { runtime }
                    if runtime.thread_id == thread_id
            )
        })
        .await;

        let persisted = ThreadRepository::get_thread(sqlite.as_ref(), &thread_id)
            .await
            .expect("thread lookup should succeed")
            .expect("thread should remain persisted");
        assert_eq!(persisted.turn_count, 1);
        assert!(persisted.token_count > 0);
    }

    #[tokio::test]
    async fn send_chat_message_recomputes_cooling_memory_after_idle() {
        let provider = Arc::new(CapturingProvider::new(
            "assistant reply that grows runtime memory",
            Duration::from_millis(25),
            64,
        ));
        let (pool, sqlite, provider_id) = test_runtime_pool_with_limit(2, provider).await;
        let (session_id, thread_id) =
            seed_chat_thread(&pool, &sqlite, provider_id, "recompute-chat-memory").await;
        let mut rx = pool.register_chat_thread(session_id, thread_id);
        let prompt = "short prompt";

        pool.send_chat_message(session_id, thread_id, prompt.to_string())
            .await
            .expect("chat message should enqueue");
        let running_memory = pool
            .collect_state()
            .runtimes
            .into_iter()
            .find(|runtime| runtime.runtime.thread_id == thread_id)
            .expect("runtime should remain tracked while running")
            .estimated_memory_bytes;
        let _ = wait_for_thread_event(&mut rx, |event| {
            matches!(
                event,
                ThreadEvent::ThreadPoolCooling { runtime }
                    if runtime.thread_id == thread_id
            )
        })
        .await;

        let runtime = pool
            .collect_state()
            .runtimes
            .into_iter()
            .find(|runtime| runtime.runtime.thread_id == thread_id)
            .expect("runtime should remain tracked");
        assert_eq!(runtime.status, ThreadRuntimeStatus::Cooling);
        assert!(
            runtime.estimated_memory_bytes > running_memory,
            "cooling memory should be recomputed from the grown thread state"
        );
    }

    #[tokio::test]
    async fn chat_runtime_admission_evicts_cooling_runtime_when_pool_is_full() {
        let provider = Arc::new(CapturingProvider::new(
            "memory pressure reply",
            Duration::from_millis(25),
            48,
        ));
        let (pool, sqlite, provider_id) = test_runtime_pool_with_limit(1, provider).await;
        let (session_a, thread_a) =
            seed_chat_thread(&pool, &sqlite, provider_id, "pressure-a").await;
        let (session_b, thread_b) =
            seed_chat_thread(&pool, &sqlite, provider_id, "pressure-b").await;
        let mut rx_a = pool.register_chat_thread(session_a, thread_a);

        pool.send_chat_message(session_a, thread_a, "first".to_string())
            .await
            .expect("first chat message should enqueue");
        let _ = wait_for_thread_event(&mut rx_a, |event| {
            matches!(
                event,
                ThreadEvent::ThreadPoolCooling { runtime }
                    if runtime.thread_id == thread_a
            )
        })
        .await;

        pool.send_chat_message(session_b, thread_b, "second".to_string())
            .await
            .expect("second chat message should enqueue");

        timeout(Duration::from_secs(5), async {
            loop {
                let runtime = pool
                    .collect_state()
                    .runtimes
                    .into_iter()
                    .find(|runtime| runtime.runtime.thread_id == thread_a)
                    .expect("first runtime should remain tracked");
                if runtime.status == ThreadRuntimeStatus::Evicted {
                    assert_eq!(
                        runtime.last_reason,
                        Some(ThreadPoolEventReason::MemoryPressure)
                    );
                    break;
                }
                sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .expect("cooling runtime should be evicted under memory pressure");
    }

    #[tokio::test]
    async fn execute_job_respects_thread_pool_capacity() {
        let provider = Arc::new(CapturingProvider::new(
            "job execution reply",
            Duration::from_millis(150),
            40,
        ));
        let (pool, _sqlite, _provider_id) = test_runtime_pool_with_limit(1, provider).await;
        let pool = Arc::new(pool);
        let request_a = super::test_request("job-capacity-a");
        let request_b = super::test_request("job-capacity-b");
        let thread_a = pool
            .enqueue_job(request_a.clone())
            .await
            .expect("first enqueue should succeed");
        let thread_b = pool
            .enqueue_job(request_b.clone())
            .await
            .expect("second enqueue should succeed");
        let (pipe_tx, _pipe_rx) = broadcast::channel(64);
        let (control_tx, _control_rx) = mpsc::unbounded_channel();

        let first = {
            let pool = Arc::clone(&pool);
            let pipe_tx = pipe_tx.clone();
            let control_tx = control_tx.clone();
            tokio::spawn(async move {
                pool.execute_job(
                    request_a,
                    thread_a,
                    pipe_tx,
                    control_tx,
                    TurnCancellation::new(),
                )
                .await
            })
        };
        let second = {
            let pool = Arc::clone(&pool);
            let pipe_tx = pipe_tx.clone();
            let control_tx = control_tx.clone();
            tokio::spawn(async move {
                pool.execute_job(
                    request_b,
                    thread_b,
                    pipe_tx,
                    control_tx,
                    TurnCancellation::new(),
                )
                .await
            })
        };

        sleep(Duration::from_millis(40)).await;

        let snapshot = pool.collect_metrics();
        assert_eq!(snapshot.running_threads, 1);
        assert_eq!(snapshot.queued_threads, 1);
        assert_eq!(snapshot.active_threads, 1);

        let first = timeout(Duration::from_secs(5), first)
            .await
            .expect("first job should complete once capacity frees")
            .expect("first job should join");
        let second = timeout(Duration::from_secs(5), second)
            .await
            .expect("second job should complete once capacity frees")
            .expect("second job should join");
        assert!(first.success);
        assert!(second.success);
    }

    #[tokio::test]
    async fn attach_chat_runtime_returns_error_when_runtime_was_removed_mid_load() {
        use argus_agent::ThreadBuilder;

        let provider = Arc::new(CapturingProvider::new(
            "attach race reply",
            Duration::from_millis(5),
            8,
        ));
        let pool = ThreadPool::test_pool();
        let session_id = SessionId::new();
        let thread_id = ThreadId::new();
        let thread = Arc::new(RwLock::new(
            ThreadBuilder::new()
                .id(thread_id)
                .session_id(session_id)
                .agent_record(Arc::new(AgentRecord {
                    id: AgentId::new(7),
                    display_name: "Attach Race Agent".to_string(),
                    description: "Used to test load/delete races".to_string(),
                    version: "1.0.0".to_string(),
                    provider_id: Some(ProviderId::new(1)),
                    model_id: Some("capturing".to_string()),
                    system_prompt: "You are a race test agent.".to_string(),
                    tool_names: vec![],
                    max_tokens: None,
                    temperature: None,
                    thinking_config: Some(ThinkingConfig::enabled()),
                    parent_agent_id: None,
                    agent_type: AgentType::Standard,
                }))
                .provider(provider)
                .compactor(super::noop_compactor())
                .build()
                .expect("thread should build"),
        ));
        let runtime_rx = {
            let guard = thread.read().await;
            guard.subscribe()
        };

        pool.register_chat_thread(session_id, thread_id);
        assert!(pool.remove_runtime(&thread_id));

        let result = pool
            .attach_chat_runtime(thread_id, session_id, thread, runtime_rx)
            .await;

        assert!(
            result.is_err(),
            "removed runtime should fail to attach cleanly"
        );
    }

    #[tokio::test]
    async fn remove_runtime_unloads_chat_runtime_and_allows_reload() {
        let provider = Arc::new(CapturingProvider::new(
            "reload after remove",
            Duration::from_millis(25),
            16,
        ));
        let (pool, sqlite, provider_id) = test_runtime_pool_with_limit(2, provider).await;
        let (session_id, thread_id) =
            seed_chat_thread(&pool, &sqlite, provider_id, "remove-runtime-reload").await;
        let thread = pool
            .ensure_chat_runtime(session_id, thread_id)
            .await
            .expect("chat runtime should load");
        let weak = Arc::downgrade(&thread);
        drop(thread);

        assert!(pool.remove_runtime(&thread_id));
        wait_for_thread_drop(&weak).await;

        let mut rx = pool.register_chat_thread(session_id, thread_id);
        pool.send_chat_message(session_id, thread_id, "reload".to_string())
            .await
            .expect("reloaded chat runtime should accept messages");
        let _ = wait_for_thread_event(&mut rx, |event| {
            matches!(
                event,
                ThreadEvent::ThreadPoolCooling { runtime }
                    if runtime.thread_id == thread_id
            )
        })
        .await;
    }

    #[tokio::test]
    async fn evict_chat_if_idle_unloads_chat_runtime() {
        let provider = Arc::new(CapturingProvider::new(
            "evict after cooling",
            Duration::from_millis(25),
            16,
        ));
        let (pool, sqlite, provider_id) = test_runtime_pool_with_limit(2, provider).await;
        let (session_id, thread_id) =
            seed_chat_thread(&pool, &sqlite, provider_id, "evict-chat-runtime").await;
        let mut rx = pool.register_chat_thread(session_id, thread_id);
        let thread = pool
            .ensure_chat_runtime(session_id, thread_id)
            .await
            .expect("chat runtime should load");
        let weak = Arc::downgrade(&thread);
        drop(thread);

        pool.send_chat_message(session_id, thread_id, "cool then evict".to_string())
            .await
            .expect("chat message should enqueue");
        let _ = wait_for_thread_event(&mut rx, |event| {
            matches!(
                event,
                ThreadEvent::ThreadPoolCooling { runtime }
                    if runtime.thread_id == thread_id
            )
        })
        .await;

        pool.evict_chat_if_idle(&thread_id)
            .expect("cooling chat runtime should evict");
        wait_for_thread_drop(&weak).await;
    }

    #[tokio::test]
    async fn ensure_chat_runtime_is_single_flight() {
        let provider = Arc::new(CapturingProvider::new(
            "single-flight chat reply",
            Duration::from_millis(5),
            12,
        ));
        let resolve_calls = Arc::new(AtomicUsize::new(0));
        let (pool, sqlite, provider_id) = test_runtime_pool_with_resolver(
            2,
            Arc::new(CountingDelayedProviderResolver::new(
                provider,
                Duration::from_millis(75),
                Arc::clone(&resolve_calls),
            )),
        )
        .await;
        let (session_id, thread_id) =
            seed_chat_thread(&pool, &sqlite, provider_id, "single-flight-chat").await;

        let (first, second) = tokio::join!(
            pool.ensure_chat_runtime(session_id, thread_id),
            pool.ensure_chat_runtime(session_id, thread_id)
        );
        let first = first.expect("first chat load should succeed");
        let second = second.expect("second chat load should succeed");

        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(resolve_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn ensure_job_runtime_is_single_flight() {
        let provider = Arc::new(CapturingProvider::new(
            "single-flight job reply",
            Duration::from_millis(5),
            12,
        ));
        let resolve_calls = Arc::new(AtomicUsize::new(0));
        let (pool, _sqlite, _provider_id) = test_runtime_pool_with_resolver(
            2,
            Arc::new(CountingDelayedProviderResolver::new(
                provider,
                Duration::from_millis(75),
                Arc::clone(&resolve_calls),
            )),
        )
        .await;
        let request = super::test_request("job-single-flight");
        let execution_thread_id = pool
            .enqueue_job(request.clone())
            .await
            .expect("enqueue should succeed");
        let request_a = request.clone();
        let request_b = request;

        let (first, second) = tokio::join!(
            pool.ensure_job_runtime(&request_a, execution_thread_id),
            pool.ensure_job_runtime(&request_b, execution_thread_id)
        );
        let first = first.expect("first job load should succeed");
        let second = second.expect("second job load should succeed");

        assert!(Arc::ptr_eq(&first, &second));
        assert_eq!(resolve_calls.load(Ordering::SeqCst), 1);
    }

    #[tokio::test]
    async fn recover_thread_state_from_sparse_turn_files_fails_on_missing_turns() {
        let trace_dir = unique_trace_dir("argus-thread-pool-chat-recovery");
        let session_id = SessionId::new();
        let thread_id = ThreadId::new();
        let base_dir = trace_dir
            .join(session_id.to_string())
            .join(thread_id.to_string());

        write_committed_turn(&base_dir, 1, "first prompt", "first reply", 11).await;
        write_committed_turn(&base_dir, 3, "third prompt", "third reply", 33).await;

        let recovered = ThreadPool::recover_thread_state_from_trace(
            &trace_dir,
            &session_id,
            &thread_id,
            Some(3),
        )
        .await
        .expect_err("sparse traces should now fail like session recovery");

        assert!(
            recovered.to_string().contains("missing turn trace file 2")
                || recovered.to_string().contains("turn file not found"),
            "unexpected sparse recovery error: {recovered}"
        );
    }

    #[tokio::test]
    async fn recover_thread_state_from_trace_uses_turns_beyond_persisted_count_hint() {
        let trace_dir = unique_trace_dir("argus-thread-pool-trace-ahead");
        let session_id = SessionId::new();
        let thread_id = ThreadId::new();
        let base_dir = trace_dir
            .join(session_id.to_string())
            .join(thread_id.to_string());

        write_committed_turn(&base_dir, 1, "first prompt", "first reply", 11).await;
        write_committed_turn(&base_dir, 2, "second prompt", "second reply", 22).await;

        let recovered = ThreadPool::recover_thread_state_from_trace(
            &trace_dir,
            &session_id,
            &thread_id,
            Some(1),
        )
        .await
        .expect("trace recovery should discover turns beyond the persisted count hint");

        assert_eq!(recovered.turn_count, 2);
        assert!(
            recovered
                .messages
                .iter()
                .any(|message| message.content == "second reply")
        );
        assert_eq!(recovered.token_count, 22);
    }

    #[tokio::test]
    async fn recover_thread_state_from_messages_jsonl() {
        let trace_dir = unique_trace_dir("argus-thread-pool-messages-jsonl");
        let session_id = SessionId::new();
        let thread_id = ThreadId::new();
        let base_dir = trace_dir
            .join(session_id.to_string())
            .join(thread_id.to_string());

        write_committed_turn(&base_dir, 1, "first prompt", "first reply", 11).await;
        write_committed_turn(&base_dir, 2, "second prompt", "second reply", 22).await;

        let recovered = ThreadPool::recover_thread_state_from_trace(
            &trace_dir,
            &session_id,
            &thread_id,
            Some(1),
        )
        .await
        .expect("committed turn logs should recover");

        assert_eq!(recovered.turn_count, 2);
        assert_eq!(recovered.token_count, 22);
        assert_eq!(recovered.messages.len(), 4);
        assert_eq!(recovered.messages[0].content, "first prompt");
        assert_eq!(recovered.messages[3].content, "second reply");
    }

    #[tokio::test]
    async fn recover_job_thread_state_fails_on_invalid_turn_files() {
        let trace_dir = unique_trace_dir("argus-thread-pool-job-recovery");
        let thread_id = ThreadId::new();
        let base_dir = trace_dir.join(thread_id.to_string());

        write_committed_turn(&base_dir, 1, "first prompt", "first reply", 11).await;
        let turn_dir = turns_dir(&base_dir);
        std::fs::write(
            turn_messages_path(&turn_dir, 2),
            "{\"role\":\"user\",\"content\":\"broken\"}\n",
        )
        .expect("invalid turn messages should persist");
        std::fs::write(turn_meta_path(&turn_dir, 2), "{invalid-json")
            .expect("invalid turn meta should persist");

        let recovered =
            ThreadPool::recover_job_thread_state_from_trace(&trace_dir, &thread_id, Some(2))
                .await
                .expect_err("invalid turn files should fail recovery");

        assert!(
            recovered.to_string().contains("turn")
                || recovered.to_string().contains("JSON")
                || recovered.to_string().contains("invalid"),
            "unexpected invalid recovery error: {recovered}"
        );
    }

    #[tokio::test]
    async fn enqueue_job_missing_agent_fails_without_persisting_rows() {
        let (pool, sqlite) = test_persistent_pool().await;
        let _ =
            LlmProviderRepository::upsert_provider(sqlite.as_ref(), &sample_provider_record(true))
                .await
                .expect("provider upsert should succeed");

        let request = super::test_request("job-missing-agent-persist");

        let result = pool.enqueue_job(request).await;

        let error = result.expect_err("enqueue should fail for missing agent");
        assert!(
            error.to_string().contains("agent 7 not found"),
            "unexpected error: {error}"
        );
        let persisted_job =
            JobRepository::get(sqlite.as_ref(), &JobId::new("job-missing-agent-persist"))
                .await
                .expect("job lookup should succeed");
        assert!(persisted_job.is_none());
        let persisted_threads = ThreadRepository::list_threads(sqlite.as_ref(), 100)
            .await
            .expect("thread list should succeed");
        assert!(persisted_threads.is_empty());
    }

    #[tokio::test]
    async fn enqueue_job_rolls_back_thread_when_job_create_fails() {
        let (pool, thread_repository) = test_pool_with_failing_job_create().await;

        let result = pool
            .enqueue_job(super::test_request("job-create-fails"))
            .await;

        let error = result.expect_err("enqueue should fail when job create fails");
        assert!(
            error.to_string().contains(
                "failed to create job record: database query failed: simulated job create failure"
            ),
            "unexpected error: {error}"
        );
        let persisted_threads = thread_repository
            .list_threads(100)
            .await
            .expect("thread list should succeed");
        assert!(persisted_threads.is_empty());
        assert_eq!(
            thread_repository
                .deleted_threads
                .lock()
                .expect("deleted thread mutex poisoned")
                .len(),
            1
        );
    }
}
