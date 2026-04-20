//! ThreadPool for coordinating runtime residency, lifecycle, and chat delivery.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use crate::error::JobError;
use argus_agent::config::ThreadConfigBuilder;
use argus_agent::thread_trace_store::{
    ThreadTraceKind, ThreadTraceMetadata, chat_thread_base_dir, recover_thread_metadata,
};
use argus_agent::turn_log_store::recover_thread_log_state;
use argus_agent::{FilePlanStore, LlmThreadCompactor, ThreadBuilder, TraceConfig, TurnConfig};
use argus_protocol::llm::{CompletionRequest, CompletionResponse, LlmError, LlmEventStream};
use argus_protocol::{
    LlmProvider, MailboxMessage, MailboxMessageType, McpToolResolver, ProviderId, ProviderResolver,
    SessionId, ThreadControlMessage, ThreadEvent, ThreadId, ThreadMessage, ThreadPoolEventReason,
    ThreadPoolRuntimeSummary, ThreadPoolSnapshot, ThreadPoolState, ThreadRuntimeStatus,
};
use argus_repository::traits::{LlmProviderRepository, ThreadRepository};
use argus_template::TemplateManager;
use argus_tool::ToolManager;
use chrono::Utc;
use rust_decimal::Decimal;
use tokio::sync::{Mutex as AsyncMutex, OwnedSemaphorePermit, RwLock, Semaphore, broadcast};
use tokio::task::AbortHandle;

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
    forwarder_abort: Option<AbortHandle>,
    slot_permit: Option<OwnedSemaphorePermit>,
    load_mutex: Arc<AsyncMutex<()>>,
}

#[derive(Debug, Default)]
struct ThreadPoolStore {
    runtimes: HashMap<ThreadId, RuntimeEntry>,
    peak_estimated_memory_bytes: u64,
    peak_process_memory_bytes: Option<u64>,
}

#[derive(Debug, Default)]
pub(crate) struct RuntimeShutdown {
    thread: Option<Arc<RwLock<argus_agent::Thread>>>,
    forwarder_abort: Option<AbortHandle>,
}

#[derive(Debug, Clone)]
pub(crate) enum RuntimeLifecycleChange {
    Evicted(ThreadPoolRuntimeSummary),
}

type RuntimeLifecycleObserver = dyn Fn(RuntimeLifecycleChange) + Send + Sync;

impl RuntimeShutdown {
    pub(crate) fn run(self) {
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
    thread_repository: Arc<dyn ThreadRepository>,
    provider_repository: Arc<dyn LlmProviderRepository>,
}

impl ThreadPoolPersistence {
    #[must_use]
    pub fn new(
        thread_repository: Arc<dyn ThreadRepository>,
        provider_repository: Arc<dyn LlmProviderRepository>,
    ) -> Self {
        Self {
            thread_repository,
            provider_repository,
        }
    }

    pub(crate) fn thread_repository(&self) -> Arc<dyn ThreadRepository> {
        Arc::clone(&self.thread_repository)
    }

    pub(crate) fn provider_repository(&self) -> Arc<dyn LlmProviderRepository> {
        Arc::clone(&self.provider_repository)
    }
}

/// Coordinates runtime residency, lifecycle transitions, and metrics.
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
    runtime_lifecycle_observer: Arc<StdMutex<Option<Arc<RuntimeLifecycleObserver>>>>,
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
            runtime_lifecycle_observer: Arc::new(StdMutex::new(None)),
        }
    }

    pub fn set_mcp_tool_resolver(&self, resolver: Option<Arc<dyn McpToolResolver>>) {
        *self
            .mcp_tool_resolver
            .lock()
            .expect("mcp resolver mutex poisoned") = resolver;
    }

    pub(crate) fn set_runtime_lifecycle_observer(
        &self,
        observer: Option<Arc<RuntimeLifecycleObserver>>,
    ) {
        *self
            .runtime_lifecycle_observer
            .lock()
            .expect("runtime lifecycle observer mutex poisoned") = observer;
    }

    fn notify_runtime_lifecycle_observer(&self, change: RuntimeLifecycleChange) {
        let observer = self
            .runtime_lifecycle_observer
            .lock()
            .expect("runtime lifecycle observer mutex poisoned")
            .clone();
        if let Some(observer) = observer {
            observer(change);
        }
    }

    pub(crate) fn persistence(&self) -> Option<ThreadPoolPersistence> {
        self.persistence.clone()
    }

    pub(crate) fn template_manager(&self) -> Arc<TemplateManager> {
        Arc::clone(&self.template_manager)
    }

    pub(crate) fn provider_resolver(&self) -> Arc<dyn ProviderResolver> {
        Arc::clone(&self.provider_resolver)
    }

    pub(crate) fn tool_manager(&self) -> Arc<ToolManager> {
        Arc::clone(&self.tool_manager)
    }

    pub(crate) fn trace_dir(&self) -> &Path {
        &self.chat_runtime_config.trace_dir
    }

    pub(crate) fn current_mcp_tool_resolver(&self) -> Option<Arc<dyn McpToolResolver>> {
        self.mcp_tool_resolver
            .lock()
            .expect("mcp resolver mutex poisoned")
            .clone()
    }

    pub(crate) fn register_runtime(
        &self,
        thread_id: ThreadId,
        status: ThreadRuntimeStatus,
        estimated_memory_bytes: u64,
        last_active_at: Option<String>,
        recoverable: bool,
        last_reason: Option<ThreadPoolEventReason>,
        thread: Option<Arc<RwLock<argus_agent::Thread>>>,
    ) -> broadcast::Sender<ThreadEvent> {
        self.upsert_runtime_summary(
            thread_id,
            None,
            status,
            estimated_memory_bytes,
            last_active_at,
            recoverable,
            last_reason,
            thread,
        )
    }

    /// Register a chat thread in the unified pool without loading its runtime.
    pub fn register_chat_thread(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> broadcast::Receiver<ThreadEvent> {
        self.upsert_runtime_summary(
            thread_id,
            Some(session_id),
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
            .get(thread_id)
            .map(|entry| entry.sender.subscribe())
    }

    pub(crate) fn event_sender(
        &self,
        thread_id: &ThreadId,
    ) -> Option<broadcast::Sender<ThreadEvent>> {
        self.store
            .lock()
            .expect("thread-pool mutex poisoned")
            .runtimes
            .get(thread_id)
            .map(|entry| entry.sender.clone())
    }

    /// Remove a runtime from the pool registry.
    pub fn remove_runtime(&self, thread_id: &ThreadId) -> bool {
        let mut store = self.store.lock().expect("thread-pool mutex poisoned");
        let removed_entry = store.runtimes.remove(thread_id);
        let removed = removed_entry.is_some();
        if removed {
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
            .get(thread_id)
            .and_then(|entry| {
                entry
                    .summary
                    .session_id
                    .is_some()
                    .then(|| entry.thread.clone())
            })
            .flatten()
    }

    /// Return a currently loaded runtime thread, if present.
    pub fn loaded_thread(&self, thread_id: &ThreadId) -> Option<Arc<RwLock<argus_agent::Thread>>> {
        self.store
            .lock()
            .expect("thread-pool mutex poisoned")
            .runtimes
            .get(thread_id)
            .and_then(|entry| entry.thread.clone())
    }

    /// Return the current runtime summary for a thread.
    pub fn runtime_summary(&self, thread_id: &ThreadId) -> Option<ThreadPoolRuntimeSummary> {
        self.store
            .lock()
            .expect("thread-pool mutex poisoned")
            .runtimes
            .get(thread_id)
            .and_then(|entry| {
                entry
                    .summary
                    .session_id
                    .is_some()
                    .then(|| entry.summary.clone())
            })
    }

    fn route_mailbox_message(message: MailboxMessage) -> ThreadMessage {
        if matches!(message.message_type, MailboxMessageType::JobResult { .. }) {
            ThreadMessage::JobResult { message }
        } else {
            ThreadMessage::PeerMessage { message }
        }
    }

    /// Deliver a mailbox message to a runtime thread.
    pub async fn deliver_mailbox_message(
        &self,
        thread_id: ThreadId,
        message: MailboxMessage,
    ) -> Result<(), JobError> {
        let (thread, session_id) = match self.runtime_summary(&thread_id) {
            Some(summary) => {
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
            None => {
                let thread = self.loaded_thread(&thread_id).ok_or_else(|| {
                    JobError::ExecutionFailed(format!("thread {} is not loaded", thread_id))
                })?;
                (thread, None)
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
                session_id: Some(session_id),
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

        if let Some(sender) = self
            .store
            .lock()
            .expect("thread-pool mutex poisoned")
            .runtimes
            .get(&thread_id)
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
                .filter(|entry| entry.summary.session_id.is_some())
                .map(|entry| entry.summary.clone())
                .collect(),
        }
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
            session_id: Some(session_id),
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
            Some(session_id),
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

    pub(crate) fn mark_runtime_running(
        &self,
        thread_id: &ThreadId,
        estimated_memory_bytes: u64,
        started_at: String,
    ) -> Option<broadcast::Sender<ThreadEvent>> {
        let mut store = self.store.lock().expect("thread-pool mutex poisoned");
        let entry = store.runtimes.get_mut(thread_id)?;
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
            .filter(|entry| entry.summary.session_id.is_some())
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
            .filter(|entry| entry.summary.session_id.is_some())
            .filter(|entry| entry.summary.status == ThreadRuntimeStatus::Queued)
            .count() as u32;
        let running_threads = store
            .runtimes
            .values()
            .filter(|entry| entry.summary.session_id.is_some())
            .filter(|entry| entry.summary.status == ThreadRuntimeStatus::Running)
            .count() as u32;
        let cooling_threads = store
            .runtimes
            .values()
            .filter(|entry| entry.summary.session_id.is_some())
            .filter(|entry| entry.summary.status == ThreadRuntimeStatus::Cooling)
            .count() as u32;
        let resident_thread_count = store
            .runtimes
            .values()
            .filter(|entry| entry.summary.session_id.is_some())
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
                .filter(|entry| entry.summary.session_id.is_some())
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
        session_id: Option<SessionId>,
        status: ThreadRuntimeStatus,
        estimated_memory_bytes: u64,
        last_active_at: Option<String>,
        recoverable: bool,
        last_reason: Option<ThreadPoolEventReason>,
        thread: Option<Arc<RwLock<argus_agent::Thread>>>,
    ) -> broadcast::Sender<ThreadEvent> {
        let mut store = self.store.lock().expect("thread-pool mutex poisoned");
        let (sender, existing_thread, existing_forwarder_abort, existing_slot_permit, load_mutex) =
            if let Some(entry) = store.runtimes.get_mut(&thread_id) {
                (
                    entry.sender.clone(),
                    entry.thread.clone(),
                    entry.forwarder_abort.take(),
                    entry.slot_permit.take(),
                    Arc::clone(&entry.load_mutex),
                )
            } else {
                let (sender, _rx) = broadcast::channel(256);
                (sender, None, None, None, Arc::new(AsyncMutex::new(())))
            };
        store.runtimes.insert(
            thread_id,
            RuntimeEntry {
                summary: ThreadPoolRuntimeSummary {
                    thread_id,
                    session_id,
                    status,
                    estimated_memory_bytes,
                    last_active_at,
                    recoverable,
                    last_reason,
                },
                sender: sender.clone(),
                thread: thread.or(existing_thread),
                forwarder_abort: existing_forwarder_abort,
                slot_permit: existing_slot_permit,
                load_mutex,
            },
        );
        Self::refresh_peaks(&mut store);
        sender
    }

    pub(crate) fn loaded_runtime(
        &self,
        thread_id: &ThreadId,
    ) -> Option<Arc<RwLock<argus_agent::Thread>>> {
        self.store
            .lock()
            .expect("thread-pool mutex poisoned")
            .runtimes
            .get(thread_id)
            .and_then(|entry| entry.thread.clone())
    }

    pub(crate) fn runtime_load_mutex(
        &self,
        thread_id: &ThreadId,
    ) -> Result<Arc<AsyncMutex<()>>, JobError> {
        self.store
            .lock()
            .expect("thread-pool mutex poisoned")
            .runtimes
            .get(thread_id)
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

    pub(crate) async fn ensure_runtime_slot(&self, thread_id: &ThreadId) -> Result<(), JobError> {
        {
            let store = self.store.lock().expect("thread-pool mutex poisoned");
            if store
                .runtimes
                .get(thread_id)
                .and_then(|entry| entry.slot_permit.as_ref())
                .is_some()
            {
                return Ok(());
            }
        }

        let permit = self.acquire_runtime_slot().await?;
        let mut store = self.store.lock().expect("thread-pool mutex poisoned");
        let entry = store.runtimes.get_mut(thread_id).ok_or_else(|| {
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

    pub(crate) fn transition_runtime_to_cooling(
        &self,
        thread_id: &ThreadId,
        estimated_memory_bytes: Option<u64>,
    ) -> Option<(
        ThreadPoolRuntimeSummary,
        broadcast::Sender<ThreadEvent>,
        ThreadPoolSnapshot,
    )> {
        let mut store = self.store.lock().expect("thread-pool mutex poisoned");
        let entry = store.runtimes.get_mut(thread_id)?;
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

    pub(crate) fn reset_runtime_after_load_failure(
        &self,
        thread_id: &ThreadId,
        reason: ThreadPoolEventReason,
    ) {
        let mut store = self.store.lock().expect("thread-pool mutex poisoned");
        let mut shutdown = RuntimeShutdown::default();
        if let Some(entry) = store.runtimes.get_mut(thread_id) {
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

    pub(crate) async fn attach_runtime(
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
                let Some(entry) = store.runtimes.get_mut(&thread_id) else {
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
                                let Some(entry) = store.runtimes.get_mut(&thread_id) else {
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
        let Some(entry) = store.runtimes.get_mut(&thread_id) else {
            forwarder_abort.abort();
            return Err(JobError::ExecutionFailed(format!(
                "thread {} was removed while attaching",
                thread_id
            )));
        };
        entry.forwarder_abort = Some(forwarder_abort);
        Ok(())
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
            .get(thread_id)
            .map(|entry| entry.sender.clone())?;
        if runtime.session_id.is_some() {
            let _ = sender.send(ThreadEvent::ThreadPoolEvicted {
                thread_id: runtime.thread_id,
                session_id: runtime.session_id,
                reason,
            });
            let _ = sender.send(ThreadEvent::ThreadPoolMetricsUpdated { snapshot });
        }
        self.notify_runtime_lifecycle_observer(RuntimeLifecycleChange::Evicted(runtime.clone()));
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
            if runtime.session_id.is_some() {
                let _ = sender.send(ThreadEvent::ThreadPoolEvicted {
                    thread_id: runtime.thread_id,
                    session_id: runtime.session_id,
                    reason: ThreadPoolEventReason::MemoryPressure,
                });
                let _ = sender.send(ThreadEvent::ThreadPoolMetricsUpdated { snapshot });
            }
            return Some(shutdown);
        }
        if runtime.session_id.is_some() {
            let _ = sender.send(ThreadEvent::ThreadPoolCooling {
                thread_id: runtime.thread_id,
                session_id: runtime.session_id,
            });
            let _ = sender.send(ThreadEvent::ThreadPoolMetricsUpdated { snapshot });
        }
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
        let entry = store.runtimes.get_mut(thread_id)?;
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

    pub(crate) async fn recover_and_validate_metadata(
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

    pub(crate) fn build_thread_config(
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

    pub(crate) async fn hydrate_turn_log_state(
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

    pub(crate) async fn resolve_provider_with_fallback(
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

        Self::hydrate_turn_log_state(&thread, &base_dir, &thread_record.updated_at).await?;

        Ok(thread)
    }

    pub(crate) async fn estimate_thread_memory(thread: &Arc<RwLock<argus_agent::Thread>>) -> u64 {
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

    pub(crate) async fn persist_thread_stats_with_persistence(
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

    pub(crate) async fn cleanup_trace_dir(base_dir: &Path) {
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
}
