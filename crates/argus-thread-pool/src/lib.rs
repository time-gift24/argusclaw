//! ThreadPool for coordinating runtime residency, lifecycle, and generic
//! thread delivery.

use std::collections::HashMap;
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};

use argus_agent::{Thread, ThreadHandle, ThreadOwnerHandle};
use argus_protocol::{
    ThreadEvent, ThreadId, ThreadMessage, ThreadPoolEventReason, ThreadPoolSnapshot,
    ThreadRuntimeStatus,
};
use chrono::Utc;
use thiserror::Error;
use tokio::sync::{
    Mutex as AsyncMutex, Notify, OwnedMutexGuard, OwnedSemaphorePermit, Semaphore, broadcast,
};
use tokio::task::AbortHandle;

const DEFAULT_MAX_THREADS: u32 = 64;
const DEFAULT_MAX_ESTIMATED_MEMORY_BYTES: u64 = 4 * 1024 * 1024 * 1024;

#[derive(Debug, Error)]
pub enum ThreadPoolError {
    #[error("thread pool execution failed: {0}")]
    ExecutionFailed(String),
}

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ThreadPoolConfigError {
    #[error("{name} must be a positive integer")]
    InvalidPositiveInteger { name: &'static str },
    #[error("{name} must be an integer byte count or disabled")]
    InvalidMemoryBudget { name: &'static str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThreadPoolConfig {
    pub max_threads: u32,
    pub max_estimated_memory_bytes: Option<u64>,
}

impl Default for ThreadPoolConfig {
    fn default() -> Self {
        Self {
            max_threads: DEFAULT_MAX_THREADS,
            max_estimated_memory_bytes: Some(DEFAULT_MAX_ESTIMATED_MEMORY_BYTES),
        }
    }
}

impl ThreadPoolConfig {
    pub fn from_env() -> Result<Self, ThreadPoolConfigError> {
        Self::from_env_values(
            std::env::var("ARGUS_THREAD_POOL_MAX_THREADS")
                .ok()
                .as_deref(),
            std::env::var("ARGUS_THREAD_POOL_MAX_ESTIMATED_MEMORY_BYTES")
                .ok()
                .as_deref(),
        )
    }

    pub fn from_env_values(
        max_threads: Option<&str>,
        max_estimated_memory_bytes: Option<&str>,
    ) -> Result<Self, ThreadPoolConfigError> {
        Ok(Self {
            max_threads: parse_positive_u32(
                "ARGUS_THREAD_POOL_MAX_THREADS",
                max_threads,
                DEFAULT_MAX_THREADS,
            )?,
            max_estimated_memory_bytes: parse_optional_memory_budget(
                "ARGUS_THREAD_POOL_MAX_ESTIMATED_MEMORY_BYTES",
                max_estimated_memory_bytes,
                Some(DEFAULT_MAX_ESTIMATED_MEMORY_BYTES),
            )?,
        })
    }
}

fn parse_positive_u32(
    name: &'static str,
    value: Option<&str>,
    default: u32,
) -> Result<u32, ThreadPoolConfigError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(default);
    };
    let parsed = value
        .parse::<u32>()
        .map_err(|_| ThreadPoolConfigError::InvalidPositiveInteger { name })?;
    if parsed == 0 {
        return Err(ThreadPoolConfigError::InvalidPositiveInteger { name });
    }
    Ok(parsed)
}

fn parse_optional_memory_budget(
    name: &'static str,
    value: Option<&str>,
    default: Option<u64>,
) -> Result<Option<u64>, ThreadPoolConfigError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(default);
    };
    if matches!(
        value.to_ascii_lowercase().as_str(),
        "none" | "off" | "disabled"
    ) {
        return Ok(None);
    }
    let parsed = value
        .parse::<u64>()
        .map_err(|_| ThreadPoolConfigError::InvalidMemoryBudget { name })?;
    Ok(Some(parsed))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuntimeSummary {
    pub thread_id: ThreadId,
    pub status: ThreadRuntimeStatus,
    pub estimated_memory_bytes: u64,
    pub last_active_at: Option<String>,
    pub recoverable: bool,
    pub last_reason: Option<ThreadPoolEventReason>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PoolState {
    pub snapshot: ThreadPoolSnapshot,
    pub runtimes: Vec<RuntimeSummary>,
}

#[derive(Debug)]
struct RuntimeEntry {
    summary: RuntimeSummary,
    sender: broadcast::Sender<ThreadEvent>,
    thread: Option<ThreadOwnerHandle>,
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
pub struct RuntimeShutdown {
    thread: Option<ThreadOwnerHandle>,
    forwarder_abort: Option<AbortHandle>,
    slot_permit: Option<OwnedSemaphorePermit>,
}

#[derive(Debug, Clone)]
pub enum RuntimeLifecycleChange {
    Cooling(RuntimeSummary),
    Evicted(RuntimeSummary),
}

type RuntimeLifecycleObserver = dyn Fn(RuntimeLifecycleChange) + Send + Sync;
type RuntimeIdleObserverFuture = Pin<Box<dyn Future<Output = ()> + Send>>;
pub type RuntimeIdleObserver =
    dyn Fn(ThreadId, ThreadHandle, &'static str) -> RuntimeIdleObserverFuture + Send + Sync;

impl RuntimeShutdown {
    pub async fn run_and_wait(self) {
        if let Some(forwarder_abort) = self.forwarder_abort {
            forwarder_abort.abort();
        }
        if let Some(thread) = self.thread {
            let _ = thread.shutdown().await;
        }
        drop(self.slot_permit);
    }
}

/// Coordinates runtime residency, lifecycle transitions, and metrics.
pub struct ThreadPool {
    max_threads: u32,
    max_estimated_memory_bytes: Option<u64>,
    resident_slots: Arc<Semaphore>,
    admission_waiters: Arc<AtomicUsize>,
    admission_notify: Arc<Notify>,
    store: Arc<StdMutex<ThreadPoolStore>>,
    runtime_lifecycle_observers: Arc<StdMutex<Vec<Arc<RuntimeLifecycleObserver>>>>,
}

impl std::fmt::Debug for ThreadPool {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThreadPool")
            .field("max_threads", &self.max_threads)
            .finish()
    }
}

impl Default for ThreadPool {
    fn default() -> Self {
        Self::with_config(ThreadPoolConfig::default())
    }
}

impl ThreadPool {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_config(config: ThreadPoolConfig) -> Self {
        Self {
            max_threads: config.max_threads,
            max_estimated_memory_bytes: config.max_estimated_memory_bytes,
            resident_slots: Arc::new(Semaphore::new(config.max_threads as usize)),
            admission_waiters: Arc::new(AtomicUsize::new(0)),
            admission_notify: Arc::new(Notify::new()),
            store: Arc::new(StdMutex::new(ThreadPoolStore::default())),
            runtime_lifecycle_observers: Arc::new(StdMutex::new(Vec::new())),
        }
    }

    pub fn add_runtime_lifecycle_observer(&self, observer: Arc<RuntimeLifecycleObserver>) {
        self.runtime_lifecycle_observers
            .lock()
            .expect("runtime lifecycle observer mutex poisoned")
            .push(observer);
    }

    fn notify_runtime_lifecycle_observers(&self, change: RuntimeLifecycleChange) {
        let observers = self
            .runtime_lifecycle_observers
            .lock()
            .expect("runtime lifecycle observer mutex poisoned")
            .clone();
        for observer in observers {
            observer(change.clone());
        }
    }

    pub fn subscribe(&self, thread_id: &ThreadId) -> Option<broadcast::Receiver<ThreadEvent>> {
        self.store
            .lock()
            .expect("thread-pool mutex poisoned")
            .runtimes
            .get(thread_id)
            .map(|entry| entry.sender.subscribe())
    }

    pub fn emit_observer_event(&self, thread_id: &ThreadId, event: ThreadEvent) -> bool {
        if !matches!(
            event,
            ThreadEvent::ThreadPoolCooling { .. }
                | ThreadEvent::ThreadPoolEvicted { .. }
                | ThreadEvent::ThreadPoolMetricsUpdated { .. }
                | ThreadEvent::ThreadPoolStarted { .. }
                | ThreadEvent::MailboxMessageQueued { .. }
                | ThreadEvent::JobRuntimeUpdated { .. }
                | ThreadEvent::JobRuntimeEvicted { .. }
                | ThreadEvent::JobRuntimeMetricsUpdated { .. }
        ) {
            return false;
        }
        let Some(sender) = self
            .store
            .lock()
            .expect("thread-pool mutex poisoned")
            .runtimes
            .get(thread_id)
            .map(|entry| entry.sender.clone())
        else {
            return false;
        };
        sender.send(event).is_ok()
    }

    pub async fn remove_runtime(&self, thread_id: &ThreadId) -> bool {
        let Some(_load_guard) = self.lock_runtime_load(thread_id).await else {
            return false;
        };
        let shutdown = {
            let mut store = self.store.lock().expect("thread-pool mutex poisoned");
            let Some(entry) = store.runtimes.get_mut(thread_id) else {
                return false;
            };
            Self::take_runtime_shutdown(entry)
        };
        shutdown.run_and_wait().await;

        let mut store = self.store.lock().expect("thread-pool mutex poisoned");
        let removed = store.runtimes.remove(thread_id).is_some();
        if removed {
            Self::refresh_peaks(&mut store);
            self.admission_notify.notify_waiters();
        }
        removed
    }

    pub fn loaded_thread(&self, thread_id: &ThreadId) -> Option<ThreadHandle> {
        self.store
            .lock()
            .expect("thread-pool mutex poisoned")
            .runtimes
            .get(thread_id)
            .and_then(|entry| entry.thread.as_ref().map(ThreadOwnerHandle::observer))
    }

    pub fn loaded_runtime(&self, thread_id: &ThreadId) -> Option<ThreadHandle> {
        self.loaded_thread(thread_id)
    }

    fn loaded_owner(&self, thread_id: &ThreadId) -> Option<ThreadOwnerHandle> {
        self.store
            .lock()
            .expect("thread-pool mutex poisoned")
            .runtimes
            .get(thread_id)
            .and_then(|entry| entry.thread.clone())
    }

    async fn lock_runtime_load(&self, thread_id: &ThreadId) -> Option<OwnedMutexGuard<()>> {
        let load_mutex = self
            .store
            .lock()
            .expect("thread-pool mutex poisoned")
            .runtimes
            .get(thread_id)
            .map(|entry| Arc::clone(&entry.load_mutex))?;
        Some(load_mutex.lock_owned().await)
    }

    pub fn is_runtime_resident(&self, thread_id: &ThreadId) -> bool {
        self.store
            .lock()
            .expect("thread-pool mutex poisoned")
            .runtimes
            .get(thread_id)
            .and_then(|entry| entry.slot_permit.as_ref())
            .is_some()
    }

    pub fn runtime_summary(&self, thread_id: &ThreadId) -> Option<RuntimeSummary> {
        self.store
            .lock()
            .expect("thread-pool mutex poisoned")
            .runtimes
            .get(thread_id)
            .map(|entry| entry.summary.clone())
    }

    pub fn collect_state(&self) -> PoolState {
        let store = self.store.lock().expect("thread-pool mutex poisoned");
        PoolState {
            snapshot: Self::collect_metrics_from_store(self.max_threads, &store),
            runtimes: store
                .runtimes
                .values()
                .map(|entry| entry.summary.clone())
                .collect(),
        }
    }

    pub fn collect_metrics(&self) -> ThreadPoolSnapshot {
        let store = self.store.lock().expect("thread-pool mutex poisoned");
        Self::collect_metrics_from_store(self.max_threads, &store)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn register_runtime(
        &self,
        thread_id: ThreadId,
        status: ThreadRuntimeStatus,
        estimated_memory_bytes: u64,
        last_active_at: Option<String>,
        recoverable: bool,
        last_reason: Option<ThreadPoolEventReason>,
        thread: Option<ThreadOwnerHandle>,
    ) -> broadcast::Sender<ThreadEvent> {
        self.upsert_runtime_summary(
            thread_id,
            status,
            estimated_memory_bytes,
            last_active_at,
            recoverable,
            last_reason,
            thread,
        )
    }

    pub fn runtime_load_mutex(
        &self,
        thread_id: &ThreadId,
    ) -> Result<Arc<AsyncMutex<()>>, ThreadPoolError> {
        self.store
            .lock()
            .expect("thread-pool mutex poisoned")
            .runtimes
            .get(thread_id)
            .map(|entry| Arc::clone(&entry.load_mutex))
            .ok_or_else(|| {
                ThreadPoolError::ExecutionFailed(format!("thread {} is not registered", thread_id))
            })
    }

    pub async fn ensure_runtime_slot(&self, thread_id: &ThreadId) -> Result<(), ThreadPoolError> {
        let candidate_estimated_memory_bytes = {
            let store = self.store.lock().expect("thread-pool mutex poisoned");
            let entry = store.runtimes.get(thread_id).ok_or_else(|| {
                ThreadPoolError::ExecutionFailed(format!("thread {} is not registered", thread_id))
            })?;
            if entry.slot_permit.is_some() {
                return Ok(());
            }
            entry.summary.estimated_memory_bytes
        };

        let permit = self
            .acquire_runtime_slot(candidate_estimated_memory_bytes)
            .await?;
        let mut store = self.store.lock().expect("thread-pool mutex poisoned");
        let entry = store.runtimes.get_mut(thread_id).ok_or_else(|| {
            ThreadPoolError::ExecutionFailed(format!("thread {} is not registered", thread_id))
        })?;
        if entry.slot_permit.is_none() {
            entry.slot_permit = Some(permit);
            Self::refresh_peaks(&mut store);
        }
        Ok(())
    }

    async fn acquire_runtime_slot(
        &self,
        candidate_estimated_memory_bytes: u64,
    ) -> Result<OwnedSemaphorePermit, ThreadPoolError> {
        loop {
            if !self.memory_budget_allows(candidate_estimated_memory_bytes) {
                if self
                    .evict_oldest_cooling_runtime(ThreadPoolEventReason::MemoryPressure)
                    .await
                    .is_some()
                {
                    continue;
                }
                self.wait_for_admission_signal().await;
                continue;
            }

            match Arc::clone(&self.resident_slots).try_acquire_owned() {
                Ok(permit) => {
                    if self.memory_budget_allows(candidate_estimated_memory_bytes) {
                        return Ok(permit);
                    }
                    drop(permit);
                    continue;
                }
                Err(tokio::sync::TryAcquireError::Closed) => {
                    return Err(ThreadPoolError::ExecutionFailed(
                        "thread pool capacity manager closed".to_string(),
                    ));
                }
                Err(tokio::sync::TryAcquireError::NoPermits) => {}
            }

            if self
                .evict_oldest_cooling_runtime(ThreadPoolEventReason::MemoryPressure)
                .await
                .is_some()
            {
                continue;
            }

            self.admission_waiters.fetch_add(1, Ordering::SeqCst);
            let permit = Arc::clone(&self.resident_slots)
                .acquire_owned()
                .await
                .map_err(|_| {
                    ThreadPoolError::ExecutionFailed(
                        "thread pool capacity manager closed".to_string(),
                    )
                });
            self.admission_waiters.fetch_sub(1, Ordering::SeqCst);
            let permit = permit?;
            if self.memory_budget_allows(candidate_estimated_memory_bytes) {
                return Ok(permit);
            }
            drop(permit);
        }
    }

    async fn wait_for_admission_signal(&self) {
        self.admission_waiters.fetch_add(1, Ordering::SeqCst);
        self.admission_notify.notified().await;
        self.admission_waiters.fetch_sub(1, Ordering::SeqCst);
    }

    fn memory_budget_allows(&self, candidate_estimated_memory_bytes: u64) -> bool {
        let Some(max_estimated_memory_bytes) = self.max_estimated_memory_bytes else {
            return true;
        };
        let store = self.store.lock().expect("thread-pool mutex poisoned");
        Self::total_estimated_memory(&store).saturating_add(candidate_estimated_memory_bytes)
            <= max_estimated_memory_bytes
    }

    pub fn mark_runtime_running(
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

    pub async fn set_runtime_title(
        &self,
        thread_id: &ThreadId,
        title: Option<String>,
    ) -> Result<(), ThreadPoolError> {
        let thread = self.loaded_owner(thread_id).ok_or_else(|| {
            ThreadPoolError::ExecutionFailed(format!("thread {} is not loaded", thread_id))
        })?;
        thread
            .set_title(title)
            .await
            .map_err(|error| ThreadPoolError::ExecutionFailed(error.to_string()))
    }

    pub async fn set_runtime_provider(
        &self,
        thread_id: &ThreadId,
        provider: Arc<dyn argus_protocol::llm::LlmProvider>,
    ) -> Result<(), ThreadPoolError> {
        let thread = self.loaded_owner(thread_id).ok_or_else(|| {
            ThreadPoolError::ExecutionFailed(format!("thread {} is not loaded", thread_id))
        })?;
        thread
            .set_provider(provider)
            .await
            .map_err(|error| ThreadPoolError::ExecutionFailed(error.to_string()))
    }

    pub fn transition_runtime_to_cooling(
        &self,
        thread_id: &ThreadId,
        estimated_memory_bytes: Option<u64>,
    ) -> Option<(
        RuntimeSummary,
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
        drop(store);
        self.admission_notify.notify_waiters();
        self.notify_runtime_lifecycle_observers(RuntimeLifecycleChange::Cooling(runtime.clone()));
        Some((runtime, sender, snapshot))
    }

    pub async fn reset_runtime_after_load_failure(
        &self,
        thread_id: &ThreadId,
        reason: ThreadPoolEventReason,
    ) {
        let shutdown = {
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
                self.admission_notify.notify_waiters();
            }
            shutdown
        };
        shutdown.run_and_wait().await;
    }

    pub async fn attach_runtime(
        &self,
        thread_id: ThreadId,
        thread: ThreadOwnerHandle,
        runtime_rx: &mut broadcast::Receiver<ThreadEvent>,
        runtime_label: &'static str,
        cool_on_idle: bool,
        idle_observer: Option<Arc<RuntimeIdleObserver>>,
    ) -> Result<(), ThreadPoolError> {
        let estimated_memory_bytes = Self::estimate_thread_memory(&thread.observer());
        let (sender, replaced_runtime) = {
            let mut store = self.store.lock().expect("thread-pool mutex poisoned");
            let (sender, replaced_runtime) = {
                let Some(entry) = store.runtimes.get_mut(&thread_id) else {
                    return Err(ThreadPoolError::ExecutionFailed(format!(
                        "thread {} was removed while loading",
                        thread_id
                    )));
                };
                let replaced_runtime = if entry
                    .thread
                    .as_ref()
                    .is_some_and(|existing| !existing.same_runtime(&thread))
                {
                    Self::take_runtime_shutdown(entry)
                } else {
                    RuntimeShutdown::default()
                };
                (entry.sender.clone(), replaced_runtime)
            };
            (sender, replaced_runtime)
        };
        replaced_runtime.run_and_wait().await;
        {
            let mut store = self.store.lock().expect("thread-pool mutex poisoned");
            let Some(entry) = store.runtimes.get_mut(&thread_id) else {
                return Err(ThreadPoolError::ExecutionFailed(format!(
                    "thread {} was removed while attaching",
                    thread_id
                )));
            };
            entry.summary.status = ThreadRuntimeStatus::Inactive;
            entry.summary.estimated_memory_bytes = estimated_memory_bytes;
            entry.summary.last_active_at = Some(Utc::now().to_rfc3339());
            entry.summary.last_reason = None;
            entry.thread = Some(thread.clone());
            entry.forwarder_abort = None;
            Self::refresh_peaks(&mut store);
        }
        let store = Arc::clone(&self.store);
        let max_threads = self.max_threads;
        let admission_waiters = Arc::clone(&self.admission_waiters);
        let admission_notify = Arc::clone(&self.admission_notify);
        let lifecycle_observers = Arc::clone(&self.runtime_lifecycle_observers);

        let mut runtime_rx = runtime_rx.resubscribe();
        let thread_for_metrics = thread.observer();
        let forwarder = tokio::spawn(async move {
            loop {
                match runtime_rx.recv().await {
                    Ok(event) => {
                        let _ = sender.send(event.clone());
                        if cool_on_idle && matches!(event, ThreadEvent::Idle { .. }) {
                            if !ThreadPool::await_runtime_idle_settle(&thread_for_metrics).await {
                                continue;
                            }
                            if let Some(observer) = idle_observer.clone() {
                                observer(thread_id, thread_for_metrics.clone(), runtime_label)
                                    .await;
                            }
                            let estimated_memory_bytes =
                                ThreadPool::estimate_thread_memory(&thread_for_metrics);

                            let (runtime, shutdown) = ThreadPool::cool_or_evict_after_idle(
                                &store,
                                max_threads,
                                &admission_waiters,
                                &admission_notify,
                                &thread_id,
                                estimated_memory_bytes,
                            )
                            .await;

                            {
                                let observers = lifecycle_observers
                                    .lock()
                                    .expect("runtime lifecycle observer mutex poisoned")
                                    .clone();
                                let change = match runtime.status {
                                    ThreadRuntimeStatus::Evicted => {
                                        RuntimeLifecycleChange::Evicted(runtime.clone())
                                    }
                                    ThreadRuntimeStatus::Cooling => {
                                        RuntimeLifecycleChange::Cooling(runtime.clone())
                                    }
                                    _ => continue,
                                };
                                for observer in observers {
                                    observer(change.clone());
                                }
                            }

                            if let Some(shutdown) = shutdown {
                                shutdown.run_and_wait().await;
                            }
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                }
            }
        });
        let forwarder_abort = forwarder.abort_handle();
        {
            let mut store = self.store.lock().expect("thread-pool mutex poisoned");
            let Some(entry) = store.runtimes.get_mut(&thread_id) else {
                forwarder_abort.abort();
                return Err(ThreadPoolError::ExecutionFailed(format!(
                    "thread {} was removed while attaching",
                    thread_id
                )));
            };
            entry.forwarder_abort = Some(forwarder_abort);
        }
        self.evict_cooling_runtimes_until_memory_fits(0).await;
        Ok(())
    }

    pub async fn load_runtime_with_builder<F, Fut>(
        &self,
        thread_id: ThreadId,
        runtime_label: &'static str,
        cool_on_idle: bool,
        idle_observer: Option<Arc<RuntimeIdleObserver>>,
        recoverable: bool,
        build_runtime: F,
    ) -> Result<ThreadHandle, ThreadPoolError>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Thread, ThreadPoolError>> + Send,
    {
        if let Some(thread) = self.loaded_runtime(&thread_id) {
            return Ok(thread);
        }

        let initial_estimated_memory_bytes = self
            .runtime_summary(&thread_id)
            .map_or(0, |summary| summary.estimated_memory_bytes);

        self.register_runtime(
            thread_id,
            ThreadRuntimeStatus::Loading,
            initial_estimated_memory_bytes,
            Some(Utc::now().to_rfc3339()),
            recoverable,
            None,
            None,
        );

        let load_mutex = self.runtime_load_mutex(&thread_id)?;
        let _load_guard = load_mutex.lock().await;
        if let Some(thread) = self.loaded_runtime(&thread_id) {
            return Ok(thread);
        }

        self.ensure_runtime_slot(&thread_id).await?;

        let thread = match build_runtime().await {
            Ok(thread) => thread,
            Err(error) => {
                self.reset_runtime_after_load_failure(
                    &thread_id,
                    ThreadPoolEventReason::ExecutionFailed,
                )
                .await;
                return Err(error);
            }
        };
        let thread = thread
            .spawn_runtime()
            .map_err(|error| ThreadPoolError::ExecutionFailed(error.to_string()))?;
        let mut runtime_rx = thread.observer().subscribe();
        if let Err(error) = self
            .attach_runtime(
                thread_id,
                thread.clone(),
                &mut runtime_rx,
                runtime_label,
                cool_on_idle,
                idle_observer,
            )
            .await
        {
            self.reset_runtime_after_load_failure(
                &thread_id,
                ThreadPoolEventReason::ExecutionFailed,
            )
            .await;
            return Err(error);
        }

        Ok(thread.observer())
    }

    pub async fn evict_runtime(
        &self,
        thread_id: &ThreadId,
        reason: ThreadPoolEventReason,
    ) -> Option<RuntimeSummary> {
        let _load_guard = self.lock_runtime_load(thread_id).await?;
        let (runtime, _snapshot, shutdown) = Self::evict_runtime_from_shared_store(
            &self.store,
            self.max_threads,
            thread_id,
            reason,
        )?;
        self.admission_notify.notify_waiters();
        shutdown.run_and_wait().await;
        self.notify_runtime_lifecycle_observers(RuntimeLifecycleChange::Evicted(runtime.clone()));
        Some(runtime)
    }

    pub async fn deliver_thread_message(
        &self,
        thread_id: ThreadId,
        message: ThreadMessage,
    ) -> Result<(), ThreadPoolError> {
        let thread = self.loaded_thread(&thread_id).ok_or_else(|| {
            ThreadPoolError::ExecutionFailed(format!("thread {} is not loaded", thread_id))
        })?;
        thread
            .send_message(message)
            .map_err(|error| ThreadPoolError::ExecutionFailed(error.to_string()))
    }

    #[allow(clippy::too_many_arguments)]
    fn upsert_runtime_summary(
        &self,
        thread_id: ThreadId,
        status: ThreadRuntimeStatus,
        estimated_memory_bytes: u64,
        last_active_at: Option<String>,
        recoverable: bool,
        last_reason: Option<ThreadPoolEventReason>,
        thread: Option<ThreadOwnerHandle>,
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
                summary: RuntimeSummary {
                    thread_id,
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

    async fn evict_oldest_cooling_runtime(
        &self,
        reason: ThreadPoolEventReason,
    ) -> Option<RuntimeSummary> {
        let candidate = {
            let store = self.store.lock().expect("thread-pool mutex poisoned");
            store
                .runtimes
                .values()
                .filter(|entry| entry.summary.status == ThreadRuntimeStatus::Cooling)
                .min_by_key(|entry| entry.summary.last_active_at.clone())
                .map(|entry| entry.summary.thread_id)
        }?;
        self.evict_runtime(&candidate, reason).await
    }

    async fn evict_cooling_runtimes_until_memory_fits(
        &self,
        candidate_estimated_memory_bytes: u64,
    ) {
        while !self.memory_budget_allows(candidate_estimated_memory_bytes) {
            if self
                .evict_oldest_cooling_runtime(ThreadPoolEventReason::MemoryPressure)
                .await
                .is_none()
            {
                break;
            }
        }
    }

    async fn cool_or_evict_after_idle(
        store: &Arc<StdMutex<ThreadPoolStore>>,
        max_threads: u32,
        admission_waiters: &AtomicUsize,
        admission_notify: &Notify,
        thread_id: &ThreadId,
        estimated_memory_bytes: u64,
    ) -> (RuntimeSummary, Option<RuntimeShutdown>) {
        let load_mutex = {
            let store_guard = store.lock().expect("thread-pool mutex poisoned");
            let entry = store_guard
                .runtimes
                .get(thread_id)
                .expect("thread must remain registered while settling idle");
            Arc::clone(&entry.load_mutex)
        };
        let _load_guard = load_mutex.lock_owned().await;
        let mut store_guard = store.lock().expect("thread-pool mutex poisoned");
        let entry = store_guard
            .runtimes
            .get_mut(thread_id)
            .expect("thread must remain registered while settling idle");
        entry.summary.status = ThreadRuntimeStatus::Cooling;
        entry.summary.estimated_memory_bytes = estimated_memory_bytes;
        entry.summary.last_active_at = Some(Utc::now().to_rfc3339());
        entry.summary.last_reason = None;
        let runtime = entry.summary.clone();
        Self::refresh_peaks(&mut store_guard);
        drop(store_guard);
        admission_notify.notify_waiters();

        if admission_waiters.load(Ordering::SeqCst) > 0
            && let Some((runtime, snapshot, shutdown)) = Self::evict_runtime_from_shared_store(
                store,
                max_threads,
                thread_id,
                ThreadPoolEventReason::MemoryPressure,
            )
        {
            let _ = snapshot;
            admission_notify.notify_waiters();
            return (runtime, Some(shutdown));
        }

        (runtime, None)
    }

    fn evict_runtime_from_shared_store(
        store: &Arc<StdMutex<ThreadPoolStore>>,
        max_threads: u32,
        thread_id: &ThreadId,
        reason: ThreadPoolEventReason,
    ) -> Option<(RuntimeSummary, ThreadPoolSnapshot, RuntimeShutdown)> {
        let mut store = store.lock().expect("thread-pool mutex poisoned");
        let entry = store.runtimes.get_mut(thread_id)?;
        if entry.summary.status != ThreadRuntimeStatus::Cooling {
            return None;
        }
        let shutdown = Self::take_runtime_shutdown(entry);
        entry.summary.status = ThreadRuntimeStatus::Evicted;
        entry.summary.last_reason = Some(reason);
        entry.summary.estimated_memory_bytes = 0;
        let runtime = entry.summary.clone();
        Self::refresh_peaks(&mut store);
        let snapshot = Self::collect_metrics_from_store(max_threads, &store);
        Some((runtime, snapshot, shutdown))
    }

    fn take_runtime_shutdown(entry: &mut RuntimeEntry) -> RuntimeShutdown {
        RuntimeShutdown {
            thread: entry.thread.take(),
            forwarder_abort: entry.forwarder_abort.take(),
            slot_permit: entry.slot_permit.take(),
        }
    }

    async fn await_runtime_idle_settle(thread: &ThreadHandle) -> bool {
        for _ in 0..64 {
            if !thread.is_turn_running() {
                return true;
            }
            tokio::task::yield_now().await;
        }

        !thread.is_turn_running()
    }

    pub fn estimate_thread_memory(thread: &ThreadHandle) -> u64 {
        thread.estimated_memory_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use argus_agent::{CompactError, CompactResult, Compactor, ThreadBuilder};
    use argus_protocol::llm::{
        ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LlmError, LlmEventStream,
        LlmProvider,
    };
    use argus_protocol::{AgentRecord, SessionId};
    use async_trait::async_trait;
    use rust_decimal::Decimal;
    use tokio::time::{Duration, sleep, timeout};

    struct FixedProvider {
        response: String,
    }

    #[async_trait]
    impl LlmProvider for FixedProvider {
        fn model_name(&self) -> &str {
            "pool-test"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> std::result::Result<CompletionResponse, LlmError> {
            Ok(CompletionResponse {
                content: Some(self.response.clone()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: 4,
                output_tokens: 2,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            })
        }

        async fn stream_complete(
            &self,
            _request: CompletionRequest,
        ) -> std::result::Result<LlmEventStream, LlmError> {
            Err(LlmError::UnsupportedCapability {
                provider: self.model_name().to_string(),
                capability: "stream_complete".to_string(),
            })
        }
    }

    struct NoopCompactor;

    struct BlockedProvider {
        response: String,
        release: Arc<tokio::sync::Notify>,
    }

    #[async_trait]
    impl Compactor for NoopCompactor {
        async fn compact(
            &self,
            _messages: &[ChatMessage],
            _token_count: u32,
        ) -> Result<Option<CompactResult>, CompactError> {
            Ok(None)
        }

        fn name(&self) -> &'static str {
            "noop"
        }
    }

    #[async_trait]
    impl LlmProvider for BlockedProvider {
        fn model_name(&self) -> &str {
            "pool-blocked-test"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> std::result::Result<CompletionResponse, LlmError> {
            self.release.notified().await;
            Ok(CompletionResponse {
                content: Some(self.response.clone()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: 4,
                output_tokens: 2,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            })
        }

        async fn stream_complete(
            &self,
            _request: CompletionRequest,
        ) -> std::result::Result<LlmEventStream, LlmError> {
            Err(LlmError::UnsupportedCapability {
                provider: self.model_name().to_string(),
                capability: "stream_complete".to_string(),
            })
        }
    }

    fn build_thread(thread_id: ThreadId, response: &str) -> Thread {
        let provider: Arc<dyn LlmProvider> = Arc::new(FixedProvider {
            response: response.to_string(),
        });
        ThreadBuilder::new()
            .id(thread_id)
            .provider(provider.clone())
            .compactor(Arc::new(NoopCompactor))
            .agent_record(Arc::new(AgentRecord {
                model_id: Some(provider.model_name().to_string()),
                ..AgentRecord::default()
            }))
            .session_id(SessionId::new())
            .build()
            .expect("test thread should build")
    }

    fn build_blocked_thread(
        thread_id: ThreadId,
        response: &str,
        release: Arc<tokio::sync::Notify>,
    ) -> Thread {
        let provider: Arc<dyn LlmProvider> = Arc::new(BlockedProvider {
            response: response.to_string(),
            release,
        });
        ThreadBuilder::new()
            .id(thread_id)
            .provider(provider.clone())
            .compactor(Arc::new(NoopCompactor))
            .agent_record(Arc::new(AgentRecord {
                model_id: Some(provider.model_name().to_string()),
                ..AgentRecord::default()
            }))
            .session_id(SessionId::new())
            .build()
            .expect("test thread should build")
    }

    async fn wait_for_status(
        pool: &ThreadPool,
        thread_id: ThreadId,
        status: ThreadRuntimeStatus,
    ) -> RuntimeSummary {
        timeout(Duration::from_secs(5), async {
            loop {
                let Some(summary) = pool.runtime_summary(&thread_id) else {
                    panic!("runtime summary should remain registered for {thread_id}");
                };
                if summary.status == status {
                    break summary;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("runtime should reach the expected status")
    }

    #[tokio::test]
    async fn default_config_expands_residency_with_memory_budget() {
        let config = ThreadPoolConfig::default();

        assert_eq!(config.max_threads, 64);
        assert_eq!(
            config.max_estimated_memory_bytes,
            Some(4 * 1024 * 1024 * 1024)
        );
    }

    #[tokio::test]
    async fn config_parses_explicit_thread_and_memory_values() {
        let config = ThreadPoolConfig::from_env_values(Some("128"), Some("8589934592"))
            .expect("explicit pool config should parse");

        assert_eq!(config.max_threads, 128);
        assert_eq!(
            config.max_estimated_memory_bytes,
            Some(8 * 1024 * 1024 * 1024)
        );
    }

    #[tokio::test]
    async fn memory_pressure_evicts_oldest_cooling_runtime_before_admission() {
        let pool = ThreadPool::with_config(ThreadPoolConfig {
            max_threads: 4,
            max_estimated_memory_bytes: Some(150),
        });
        let old_cooling = ThreadId::new();
        let new_runtime = ThreadId::new();

        pool.register_runtime(
            old_cooling,
            ThreadRuntimeStatus::Cooling,
            100,
            Some("2026-05-11T00:00:00Z".to_string()),
            true,
            None,
            None,
        );
        pool.ensure_runtime_slot(&old_cooling)
            .await
            .expect("cooling runtime should become resident");
        pool.register_runtime(
            new_runtime,
            ThreadRuntimeStatus::Loading,
            80,
            Some("2026-05-11T00:00:01Z".to_string()),
            true,
            None,
            None,
        );

        pool.ensure_runtime_slot(&new_runtime)
            .await
            .expect("new runtime should be admitted after evicting cooling runtime");

        assert!(pool.is_runtime_resident(&new_runtime));
        let evicted = pool
            .runtime_summary(&old_cooling)
            .expect("evicted runtime summary should remain");
        assert_eq!(evicted.status, ThreadRuntimeStatus::Evicted);
        assert_eq!(
            evicted.last_reason,
            Some(ThreadPoolEventReason::MemoryPressure)
        );
        assert_eq!(pool.collect_metrics().estimated_memory_bytes, 80);
    }

    #[tokio::test]
    async fn memory_pressure_waits_when_only_non_cooling_runtimes_remain() {
        let pool = Arc::new(ThreadPool::with_config(ThreadPoolConfig {
            max_threads: 4,
            max_estimated_memory_bytes: Some(100),
        }));
        let running = ThreadId::new();
        let waiting = ThreadId::new();

        pool.register_runtime(
            running,
            ThreadRuntimeStatus::Running,
            90,
            Some("2026-05-11T00:00:00Z".to_string()),
            true,
            None,
            None,
        );
        pool.ensure_runtime_slot(&running)
            .await
            .expect("running runtime should become resident");
        pool.register_runtime(
            waiting,
            ThreadRuntimeStatus::Loading,
            20,
            Some("2026-05-11T00:00:01Z".to_string()),
            true,
            None,
            None,
        );

        let waiting_pool = Arc::clone(&pool);
        let wait_task =
            tokio::spawn(async move { waiting_pool.ensure_runtime_slot(&waiting).await });
        sleep(Duration::from_millis(50)).await;

        assert!(
            !wait_task.is_finished(),
            "memory pressure should wait instead of over-admitting"
        );

        pool.transition_runtime_to_cooling(&running, Some(90))
            .expect("running runtime should enter cooling");

        wait_task
            .await
            .expect("wait task should join")
            .expect("waiting runtime should be admitted after cooling eviction");
        assert!(pool.is_runtime_resident(&waiting));
        assert_eq!(
            pool.runtime_summary(&running)
                .expect("runtime summary should remain")
                .status,
            ThreadRuntimeStatus::Evicted
        );
    }

    #[tokio::test]
    async fn disabled_memory_budget_preserves_count_only_admission() {
        let pool = ThreadPool::with_config(ThreadPoolConfig {
            max_threads: 2,
            max_estimated_memory_bytes: None,
        });
        let first = ThreadId::new();
        let second = ThreadId::new();

        pool.register_runtime(
            first,
            ThreadRuntimeStatus::Running,
            1_000,
            Some("2026-05-11T00:00:00Z".to_string()),
            true,
            None,
            None,
        );
        pool.ensure_runtime_slot(&first)
            .await
            .expect("first runtime should be admitted");
        pool.register_runtime(
            second,
            ThreadRuntimeStatus::Loading,
            1_000,
            Some("2026-05-11T00:00:01Z".to_string()),
            true,
            None,
            None,
        );

        pool.ensure_runtime_slot(&second)
            .await
            .expect("disabled memory budget should not block admission");

        assert!(pool.is_runtime_resident(&first));
        assert!(pool.is_runtime_resident(&second));
    }

    #[tokio::test]
    async fn load_runtime_with_builder_reuses_loaded_runtime() {
        let pool = ThreadPool::new();
        let thread_id = ThreadId::new();
        let build_count = Arc::new(AtomicUsize::new(0));
        let thread = build_thread(thread_id, "hello");

        let first = pool
            .load_runtime_with_builder(thread_id, "test runtime", false, None, true, {
                let build_count = Arc::clone(&build_count);
                let thread = thread;
                move || {
                    build_count.fetch_add(1, Ordering::SeqCst);
                    let thread = thread;
                    async move { Ok(thread) }
                }
            })
            .await
            .expect("first load should succeed");
        let second = pool
            .load_runtime_with_builder(thread_id, "test runtime", false, None, true, {
                let build_count = Arc::clone(&build_count);
                move || {
                    build_count.fetch_add(1, Ordering::SeqCst);
                    async move {
                        Err(ThreadPoolError::ExecutionFailed(
                            "builder should not run twice".to_string(),
                        ))
                    }
                }
            })
            .await
            .expect("second load should reuse the existing runtime");

        assert!(first.same_runtime(&second));
        assert_eq!(build_count.load(Ordering::SeqCst), 1);
        assert_eq!(
            pool.runtime_summary(&thread_id)
                .expect("runtime summary should exist"),
            RuntimeSummary {
                thread_id,
                status: ThreadRuntimeStatus::Inactive,
                estimated_memory_bytes: ThreadPool::estimate_thread_memory(&first),
                last_active_at: pool
                    .runtime_summary(&thread_id)
                    .and_then(|summary| summary.last_active_at),
                recoverable: true,
                last_reason: None,
            }
        );
    }

    #[tokio::test]
    async fn deliver_thread_message_forwards_user_input_to_loaded_thread() {
        let pool = ThreadPool::new();
        let thread_id = ThreadId::new();
        let thread = pool
            .load_runtime_with_builder(thread_id, "test runtime", false, None, true, {
                let thread = build_thread(thread_id, "assistant reply");
                move || async move { Ok(thread) }
            })
            .await
            .expect("runtime load should succeed");

        pool.deliver_thread_message(
            thread_id,
            ThreadMessage::UserInput {
                content: "hello".to_string(),
                msg_override: None,
            },
        )
        .await
        .expect("user input should route through the pool");

        timeout(Duration::from_secs(5), async {
            loop {
                if thread.turn_count() == 1 {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("thread should settle the delivered turn");

        assert_eq!(thread.turn_count(), 1);
        assert!(
            thread
                .history()
                .into_iter()
                .any(|message| message.content == "assistant reply"),
            "delivered user input should reach the loaded runtime"
        );
    }

    #[tokio::test]
    async fn idle_cooling_notifies_observers_without_broadcasting_pool_metrics() {
        let pool = ThreadPool::new();
        let thread_id = ThreadId::new();
        let changes = Arc::new(StdMutex::new(Vec::new()));
        pool.add_runtime_lifecycle_observer(Arc::new({
            let changes = Arc::clone(&changes);
            move |change| {
                changes
                    .lock()
                    .expect("observer mutex poisoned")
                    .push(change);
            }
        }));

        let _thread = pool
            .load_runtime_with_builder(thread_id, "chat runtime", true, None, true, {
                let thread = build_thread(thread_id, "idle reply");
                move || async move { Ok(thread) }
            })
            .await
            .expect("runtime load should succeed");
        let mut rx = pool
            .subscribe(&thread_id)
            .expect("loaded runtime should expose an event receiver");

        pool.deliver_thread_message(
            thread_id,
            ThreadMessage::UserInput {
                content: "hello".to_string(),
                msg_override: None,
            },
        )
        .await
        .expect("user input should route through the pool");

        let summary = wait_for_status(&pool, thread_id, ThreadRuntimeStatus::Cooling).await;
        assert_eq!(summary.thread_id, thread_id);

        sleep(Duration::from_millis(50)).await;
        let recorded = changes.lock().expect("observer mutex poisoned");
        assert!(
            recorded.iter().any(|change| {
                matches!(change, RuntimeLifecycleChange::Cooling(runtime) if runtime.thread_id == thread_id)
            }),
            "idle cooling should notify lifecycle observers"
        );
        drop(recorded);

        while let Ok(event) = rx.try_recv() {
            assert!(
                !matches!(event, ThreadEvent::ThreadPoolMetricsUpdated { .. }),
                "pool-core should not emit adapted metrics events directly"
            );
        }
    }

    #[tokio::test]
    async fn evict_runtime_notifies_observers_and_unloads_thread() {
        let pool = ThreadPool::new();
        let thread_id = ThreadId::new();
        let changes = Arc::new(StdMutex::new(Vec::new()));
        pool.add_runtime_lifecycle_observer(Arc::new({
            let changes = Arc::clone(&changes);
            move |change| {
                changes
                    .lock()
                    .expect("observer mutex poisoned")
                    .push(change);
            }
        }));

        let thread = pool
            .load_runtime_with_builder(thread_id, "job runtime", false, None, true, {
                let thread = build_thread(thread_id, "evict me");
                move || async move { Ok(thread) }
            })
            .await
            .expect("runtime load should succeed");
        let cooling_memory = ThreadPool::estimate_thread_memory(&thread);
        pool.transition_runtime_to_cooling(&thread_id, Some(cooling_memory))
            .expect("runtime should enter cooling before eviction");

        let evicted = pool
            .evict_runtime(&thread_id, ThreadPoolEventReason::CoolingExpired)
            .await
            .expect("cooling runtime should evict");

        assert_eq!(evicted.status, ThreadRuntimeStatus::Evicted);
        assert_eq!(
            evicted.last_reason,
            Some(ThreadPoolEventReason::CoolingExpired)
        );
        assert!(pool.loaded_thread(&thread_id).is_none());
        assert!(
            changes.lock().expect("observer mutex poisoned").iter().any(|change| {
                matches!(change, RuntimeLifecycleChange::Evicted(runtime) if runtime.thread_id == thread_id)
            }),
            "eviction should notify lifecycle observers"
        );
    }

    #[tokio::test]
    async fn emit_observer_event_rejects_owner_authored_terminal_events() {
        let pool = ThreadPool::new();
        let thread_id = ThreadId::new();
        pool.register_runtime(
            thread_id,
            ThreadRuntimeStatus::Inactive,
            0,
            None,
            true,
            None,
            None,
        );
        let _rx = pool
            .subscribe(&thread_id)
            .expect("registered runtime should expose an event receiver");

        assert!(!pool.emit_observer_event(
            &thread_id,
            ThreadEvent::Idle {
                thread_id: thread_id.to_string(),
            }
        ));
        assert!(pool.emit_observer_event(
            &thread_id,
            ThreadEvent::ThreadPoolMetricsUpdated {
                snapshot: pool.collect_metrics(),
            }
        ));
    }

    #[tokio::test]
    async fn remove_runtime_waits_for_inflight_load_to_finish_without_orphaning_owner() {
        let pool = Arc::new(ThreadPool::new());
        let thread_id = ThreadId::new();
        let build_started = Arc::new(tokio::sync::Notify::new());
        let release_build = Arc::new(tokio::sync::Notify::new());

        let load_pool = Arc::clone(&pool);
        let load_task = tokio::spawn({
            let build_started = Arc::clone(&build_started);
            let release_build = Arc::clone(&release_build);
            async move {
                load_pool
                    .load_runtime_with_builder(
                        thread_id,
                        "chat runtime",
                        false,
                        None,
                        true,
                        move || {
                            let build_started = Arc::clone(&build_started);
                            let release_build = Arc::clone(&release_build);
                            async move {
                                build_started.notify_waiters();
                                release_build.notified().await;
                                Ok(build_thread(thread_id, "loaded after block"))
                            }
                        },
                    )
                    .await
            }
        });

        build_started.notified().await;
        let remove_pool = Arc::clone(&pool);
        let remove_task = tokio::spawn(async move { remove_pool.remove_runtime(&thread_id).await });
        sleep(Duration::from_millis(20)).await;
        assert!(
            !remove_task.is_finished(),
            "remove_runtime should wait for the inflight load to leave the builder phase"
        );

        release_build.notify_waiters();
        let loaded = load_task
            .await
            .expect("load task should join")
            .expect("load should complete before coordinated removal");
        assert_eq!(loaded.id(), thread_id);
        assert!(
            remove_task.await.expect("remove task should join"),
            "remove_runtime should remove the runtime once load completes"
        );
        assert!(pool.loaded_thread(&thread_id).is_none());
    }

    #[tokio::test]
    async fn remove_runtime_waits_for_shutdown_before_allowing_replacement() {
        let pool = Arc::new(ThreadPool::new());
        let thread_id = ThreadId::new();
        let release = Arc::new(tokio::sync::Notify::new());
        let original = pool
            .load_runtime_with_builder(thread_id, "chat runtime", false, None, true, {
                let thread = build_blocked_thread(thread_id, "slow reply", Arc::clone(&release));
                move || async move { Ok(thread) }
            })
            .await
            .expect("runtime load should succeed");

        pool.deliver_thread_message(
            thread_id,
            ThreadMessage::UserInput {
                content: "hello".to_string(),
                msg_override: None,
            },
        )
        .await
        .expect("message should start the runtime");

        timeout(Duration::from_secs(5), async {
            loop {
                if original.is_turn_running() {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("runtime should start running before removal");

        let removing_pool = Arc::clone(&pool);
        let remove_task =
            tokio::spawn(async move { removing_pool.remove_runtime(&thread_id).await });
        sleep(Duration::from_millis(20)).await;
        let removal_pending = !remove_task.is_finished();

        let replacement_builds = Arc::new(AtomicUsize::new(0));
        let replacement = pool
            .load_runtime_with_builder(thread_id, "chat runtime", false, None, true, {
                let replacement_builds = Arc::clone(&replacement_builds);
                move || {
                    replacement_builds.fetch_add(1, Ordering::SeqCst);
                    let thread = build_thread(thread_id, "replacement reply");
                    async move { Ok(thread) }
                }
            })
            .await
            .expect("concurrent load should reuse the shutting-down runtime");

        if removal_pending {
            assert!(
                replacement.same_runtime(&original),
                "replacement must not create a new owner while shutdown is still settling"
            );
            assert_eq!(
                replacement_builds.load(Ordering::SeqCst),
                0,
                "builder should not run while the old runtime is still being removed"
            );
        }

        release.notify_waiters();
        assert!(
            remove_task.await.expect("remove task should join"),
            "remove_runtime should finish once the old owner settles"
        );
        if replacement_builds.load(Ordering::SeqCst) == 0 {
            assert!(pool.loaded_thread(&thread_id).is_none());

            let replacement = pool
                .load_runtime_with_builder(thread_id, "chat runtime", false, None, true, {
                    let replacement_builds = Arc::clone(&replacement_builds);
                    move || {
                        replacement_builds.fetch_add(1, Ordering::SeqCst);
                        let thread = build_thread(thread_id, "replacement reply");
                        async move { Ok(thread) }
                    }
                })
                .await
                .expect("load should succeed after removal completes");

            assert_eq!(replacement_builds.load(Ordering::SeqCst), 1);
            assert!(
                !replacement.same_runtime(&original),
                "a new owner should only appear after the old runtime is fully removed"
            );
        } else {
            assert_eq!(replacement_builds.load(Ordering::SeqCst), 1);
            assert!(
                !replacement.same_runtime(&original),
                "once removal has completed, a subsequent load may create a fresh owner"
            );
        }
    }

    #[tokio::test]
    async fn evict_runtime_waits_for_shutdown_before_allowing_replacement() {
        let pool = Arc::new(ThreadPool::new());
        let thread_id = ThreadId::new();
        let release = Arc::new(tokio::sync::Notify::new());
        let original = pool
            .load_runtime_with_builder(thread_id, "chat runtime", false, None, true, {
                let thread = build_blocked_thread(thread_id, "slow reply", Arc::clone(&release));
                move || async move { Ok(thread) }
            })
            .await
            .expect("runtime load should succeed");

        pool.deliver_thread_message(
            thread_id,
            ThreadMessage::UserInput {
                content: "hello".to_string(),
                msg_override: None,
            },
        )
        .await
        .expect("message should start the runtime");

        timeout(Duration::from_secs(5), async {
            loop {
                if original.is_turn_running() {
                    break;
                }
                sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("runtime should start running before eviction");

        pool.transition_runtime_to_cooling(
            &thread_id,
            Some(ThreadPool::estimate_thread_memory(&original)),
        )
        .expect("runtime should enter cooling before eviction");

        let evicting_pool = Arc::clone(&pool);
        let evict_task = tokio::spawn(async move {
            evicting_pool
                .evict_runtime(&thread_id, ThreadPoolEventReason::CoolingExpired)
                .await
        });
        sleep(Duration::from_millis(20)).await;
        let eviction_pending = !evict_task.is_finished();

        let replacement_builds = Arc::new(AtomicUsize::new(0));
        let replacement = pool
            .load_runtime_with_builder(thread_id, "chat runtime", false, None, true, {
                let replacement_builds = Arc::clone(&replacement_builds);
                move || {
                    replacement_builds.fetch_add(1, Ordering::SeqCst);
                    let thread = build_thread(thread_id, "replacement reply");
                    async move { Ok(thread) }
                }
            })
            .await
            .expect("concurrent load should succeed");

        if eviction_pending {
            assert!(replacement.same_runtime(&original));
            assert_eq!(replacement_builds.load(Ordering::SeqCst), 0);
        }

        release.notify_waiters();
        let evicted = evict_task
            .await
            .expect("evict task should join")
            .expect("eviction should succeed");
        assert_eq!(evicted.status, ThreadRuntimeStatus::Evicted);
    }
}
