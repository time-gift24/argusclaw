//! ThreadPool for coordinating unified job and chat runtimes.

use std::collections::{HashMap, HashSet};
use std::panic::AssertUnwindSafe;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex as StdMutex};

use argus_agent::config::ThreadConfigBuilder;
use argus_agent::{
    read_jsonl_events, CompactorManager, FilePlanStore, OnTurnComplete, ThreadBuilder,
    TraceConfig, TurnBuilder, TurnConfig, TurnLogEvent, TurnOutput,
};
use argus_protocol::llm::{ChatMessage, Role};
use argus_protocol::tool::NamedTool;
use argus_protocol::{
    AgentId, ProviderId, ProviderResolver, SessionId, ThreadControlEvent, ThreadEvent, ThreadId,
    ThreadJobResult, ThreadMailbox, ThreadPoolEventReason, ThreadPoolRuntimeKind,
    ThreadPoolRuntimeRef, ThreadPoolRuntimeSummary, ThreadPoolSnapshot, ThreadPoolState,
    ThreadRuntimeStatus,
};
use argus_repository::traits::{JobRepository, LlmProviderRepository, ThreadRepository};
use argus_repository::types::{
    AgentId as RepoAgentId, JobRecord, JobType, ThreadRecord, WorkflowId, WorkflowStatus,
};
use argus_template::TemplateManager;
use argus_tool::ToolManager;
use chrono::Utc;
use futures_util::FutureExt;
use tokio::sync::{Mutex, RwLock, broadcast, mpsc};

use crate::error::JobError;
use crate::types::ThreadPoolJobRequest;

const DEFAULT_MAX_THREADS: u32 = 8;

#[derive(Debug, Clone)]
struct ChatRuntimeConfig {
    compactor_manager: Arc<CompactorManager>,
    trace_dir: PathBuf,
}

#[derive(Debug)]
struct RecoveredThreadState {
    messages: Vec<ChatMessage>,
    turn_count: u32,
    token_count: u32,
}

#[derive(Debug)]
struct RuntimeEntry {
    summary: ThreadPoolRuntimeSummary,
    sender: broadcast::Sender<ThreadEvent>,
    chat_thread: Option<Arc<RwLock<argus_agent::Thread>>>,
}

#[derive(Debug, Default)]
struct ThreadPoolStore {
    runtimes: HashMap<String, RuntimeEntry>,
    job_bindings: HashMap<String, ThreadId>,
    peak_estimated_memory_bytes: u64,
    peak_process_memory_bytes: Option<u64>,
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
        compactor_manager: Arc<CompactorManager>,
        trace_dir: PathBuf,
    ) -> Self {
        Self::with_persistence(
            template_manager,
            provider_resolver,
            tool_manager,
            compactor_manager,
            trace_dir,
            None,
        )
    }

    /// Create a thread pool with optional repository-backed persistence.
    pub fn with_persistence(
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        tool_manager: Arc<ToolManager>,
        compactor_manager: Arc<CompactorManager>,
        trace_dir: PathBuf,
        persistence: Option<ThreadPoolPersistence>,
    ) -> Self {
        Self {
            template_manager,
            provider_resolver,
            tool_manager,
            chat_runtime_config: ChatRuntimeConfig {
                compactor_manager,
                trace_dir,
            },
            persistence,
            max_threads: DEFAULT_MAX_THREADS,
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
        let removed = store.runtimes.remove(&thread_id.to_string()).is_some();
        if removed {
            store.job_bindings.retain(|_, bound_thread_id| bound_thread_id != thread_id);
            Self::refresh_peaks(&mut store);
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
            .and_then(|entry| entry.chat_thread.clone())
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
            .insert(request.job_id, thread_id);
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
            Self::estimate_chat_thread_memory(&thread).await + message.len() as u64;
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
        if let Some(thread) = self
            .store
            .lock()
            .expect("thread-pool mutex poisoned")
            .runtimes
            .get(&thread_id.to_string())
            .and_then(|entry| entry.chat_thread.clone())
        {
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

        let thread = self.build_chat_thread(session_id, thread_id).await?;
        let runtime_rx = {
            let guard = thread.read().await;
            guard.subscribe()
        };
        argus_agent::Thread::spawn_runtime_actor(Arc::clone(&thread));
        self.attach_chat_runtime(thread_id, session_id, Arc::clone(&thread), runtime_rx)
            .await;
        Ok(thread)
    }

    /// Execute an enqueued job on its bound thread runtime.
    #[allow(clippy::too_many_arguments)]
    pub async fn execute_job(
        &self,
        request: ThreadPoolJobRequest,
        execution_thread_id: ThreadId,
        pipe_tx: broadcast::Sender<ThreadEvent>,
        control_tx: mpsc::UnboundedSender<ThreadControlEvent>,
    ) -> ThreadJobResult {
        let fallback_job_id = request.job_id.clone();
        let fallback_agent_id = request.agent_id;
        let fallback_display_name = format!("Agent {}", fallback_agent_id.inner());

        let _ = self.mark_running(&request.job_id);
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

        let result = AssertUnwindSafe(self.execute_turn(
            request.originating_thread_id,
            request.job_id.clone(),
            execution_thread_id,
            request.agent_id,
            request.prompt,
            pipe_tx.clone(),
            control_tx,
        ))
        .catch_unwind()
        .await;

        let result = match result {
            Ok(result) => result,
            Err(payload) => Self::failure_result(
                fallback_job_id,
                fallback_agent_id,
                fallback_display_name,
                String::new(),
                Self::panic_message(payload),
            ),
        };

        let _ = self.mark_cooling(&request.job_id);
        let _ = pipe_tx.send(ThreadEvent::ThreadPoolCooling {
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

        result
    }

    #[allow(clippy::too_many_arguments)]
    async fn execute_turn(
        &self,
        originating_thread_id: ThreadId,
        job_id: String,
        execution_thread_id: ThreadId,
        agent_id: AgentId,
        prompt: String,
        pipe_tx: broadcast::Sender<ThreadEvent>,
        control_tx: mpsc::UnboundedSender<ThreadControlEvent>,
    ) -> ThreadJobResult {
        #[cfg(test)]
        if prompt == "__panic_thread_pool_execute_turn__" {
            panic!("thread pool panic test hook");
        }

        let default_display_name = format!("Agent {}", agent_id.inner());

        let agent_record = match self.template_manager.get(agent_id).await {
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
            Some(pid) => match self.provider_resolver.resolve(pid).await {
                Ok(provider) => provider,
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
            None => match self.provider_resolver.default_provider().await {
                Ok(provider) => provider,
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
        let tools: Vec<Arc<dyn NamedTool>> = self
            .tool_manager
            .list_ids()
            .iter()
            .filter(|name| enabled_tool_names.contains(*name))
            .filter_map(|name| self.tool_manager.get(name))
            .collect();

        let (stream_tx, _stream_rx) = broadcast::channel(256);
        let turn = match TurnBuilder::default()
            .turn_number(1)
            .thread_id(execution_thread_id.to_string())
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
            .filter(|entry| {
                matches!(
                    entry.summary.status,
                    ThreadRuntimeStatus::Loading
                        | ThreadRuntimeStatus::Queued
                        | ThreadRuntimeStatus::Running
                        | ThreadRuntimeStatus::Cooling
                )
            })
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
            .filter(|entry| {
                matches!(
                    entry.summary.status,
                    ThreadRuntimeStatus::Loading
                        | ThreadRuntimeStatus::Queued
                        | ThreadRuntimeStatus::Running
                        | ThreadRuntimeStatus::Cooling
                )
            })
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

    fn upsert_runtime_summary(
        &self,
        runtime: ThreadPoolRuntimeRef,
        status: ThreadRuntimeStatus,
        estimated_memory_bytes: u64,
        last_active_at: Option<String>,
        recoverable: bool,
        last_reason: Option<ThreadPoolEventReason>,
        chat_thread: Option<Arc<RwLock<argus_agent::Thread>>>,
    ) -> broadcast::Sender<ThreadEvent> {
        let mut store = self.store.lock().expect("thread-pool mutex poisoned");
        let runtime_key = runtime.thread_id.to_string();
        let sender = store
            .runtimes
            .get(&runtime_key)
            .map(|entry| entry.sender.clone())
            .unwrap_or_else(|| {
                let (sender, _rx) = broadcast::channel(256);
                sender
            });

        let existing_chat_thread = store
            .runtimes
            .get(&runtime_key)
            .and_then(|entry| entry.chat_thread.clone());
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
                chat_thread: chat_thread.or(existing_chat_thread),
            },
        );
        Self::refresh_peaks(&mut store);
        sender
    }

    async fn attach_chat_runtime(
        &self,
        thread_id: ThreadId,
        session_id: SessionId,
        thread: Arc<RwLock<argus_agent::Thread>>,
        mut runtime_rx: broadcast::Receiver<ThreadEvent>,
    ) {
        let estimated_memory_bytes = Self::estimate_chat_thread_memory(&thread).await;
        let sender = {
            let mut store = self.store.lock().expect("thread-pool mutex poisoned");
            let sender = {
                let entry = store
                    .runtimes
                    .get_mut(&thread_id.to_string())
                    .expect("chat runtime should be registered");
                entry.summary.status = ThreadRuntimeStatus::Inactive;
                entry.summary.estimated_memory_bytes = estimated_memory_bytes;
                entry.summary.last_active_at = Some(Utc::now().to_rfc3339());
                entry.summary.last_reason = None;
                entry.chat_thread = Some(thread);
                entry.sender.clone()
            };
            Self::refresh_peaks(&mut store);
            sender
        };
        let store = Arc::clone(&self.store);
        let max_threads = self.max_threads;

        tokio::spawn(async move {
            loop {
                match runtime_rx.recv().await {
                    Ok(event) => {
                        let _ = sender.send(event.clone());
                        if matches!(event, ThreadEvent::Idle { .. }) {
                            let snapshot = {
                                let mut store = store.lock().expect("thread-pool mutex poisoned");
                                if let Some(entry) = store.runtimes.get_mut(&thread_id.to_string()) {
                                    entry.summary.status = ThreadRuntimeStatus::Cooling;
                                    entry.summary.last_active_at =
                                        Some(Utc::now().to_rfc3339());
                                    entry.summary.last_reason = None;
                                }
                                ThreadPool::collect_metrics_from_store(max_threads, &store)
                            };
                            let _ = sender.send(ThreadEvent::ThreadPoolCooling {
                                runtime: ThreadPoolRuntimeRef {
                                    thread_id,
                                    kind: ThreadPoolRuntimeKind::Chat,
                                    session_id: Some(session_id),
                                    job_id: None,
                                },
                            });
                            let _ = sender.send(ThreadEvent::ThreadPoolMetricsUpdated { snapshot });
                        }
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                }
            }
        });
    }

    fn evict_runtime(
        &self,
        thread_id: &ThreadId,
        reason: ThreadPoolEventReason,
    ) -> Option<ThreadPoolRuntimeRef> {
        let (runtime, sender) = {
            let mut store = self.store.lock().expect("thread-pool mutex poisoned");
            let entry = store.runtimes.get_mut(&thread_id.to_string())?;
            if entry.summary.status != ThreadRuntimeStatus::Cooling {
                return None;
            }
            entry.summary.status = ThreadRuntimeStatus::Evicted;
            entry.summary.last_reason = Some(reason.clone());
            entry.summary.estimated_memory_bytes = 0;
            entry.chat_thread = None;
            (entry.summary.runtime.clone(), entry.sender.clone())
        };
        let _ = sender.send(ThreadEvent::ThreadPoolEvicted {
            runtime: runtime.clone(),
            reason,
        });
        let _ = sender.send(ThreadEvent::ThreadPoolMetricsUpdated {
            snapshot: self.collect_metrics(),
        });
        Some(runtime)
    }

    fn summarize_output(output: &TurnOutput) -> String {
        const SUMMARY_LIMIT: usize = 4000;

        for msg in output.messages.iter().rev() {
            if let ChatMessage {
                role: Role::Assistant,
                content,
                ..
            } = msg
                && !content.is_empty()
            {
                let mut chars = content.chars();
                let summary: String = chars.by_ref().take(SUMMARY_LIMIT).collect();
                return if chars.next().is_some() {
                    format!("{summary}...")
                } else {
                    content.clone()
                };
            }
        }
        format!("job completed, {} messages in turn", output.messages.len())
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

    fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
        let payload = payload.as_ref();
        let detail = payload
            .downcast_ref::<&'static str>()
            .map(|msg| (*msg).to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic payload".to_string());
        format!("job executor panicked: {detail}")
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
            .ok_or_else(|| {
                JobError::ExecutionFailed(format!("thread {} not found", thread_id))
            })?;
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
        }
        .map_err(|err| JobError::ExecutionFailed(format!("failed to resolve provider: {err}")))?;

        let trace_cfg = TraceConfig::new(true, self.chat_runtime_config.trace_dir.clone())
            .with_session_id(session_id)
            .with_turn_start(
                Some(agent_record.system_prompt.clone()),
                Some(provider.model_name().to_string()),
            );
        let mut turn_config = TurnConfig::new();
        turn_config.trace_config = Some(trace_cfg);
        turn_config.on_turn_complete =
            Some(Self::build_on_turn_complete(self.chat_runtime_config.trace_dir.clone()));
        let config = ThreadConfigBuilder::default()
            .turn_config(turn_config)
            .build()
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        let plan_store = FilePlanStore::new(
            self.chat_runtime_config.trace_dir.clone(),
            &thread_id.inner().to_string(),
        );
        let thread = ThreadBuilder::new()
            .id(thread_id)
            .session_id(session_id)
            .agent_record(Arc::new(agent_record))
            .title(thread_record.title.clone())
            .provider(provider)
            .tool_manager(self.tool_manager.clone())
            .compactor(self.chat_runtime_config.compactor_manager.default_compactor().clone())
            .plan_store(plan_store)
            .config(config)
            .build()
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        let thread = Arc::new(RwLock::new(thread));

        let updated_at = chrono::DateTime::parse_from_rfc3339(&thread_record.updated_at)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .unwrap_or_else(|_| chrono::Utc::now());
        let recovered = Self::recover_thread_state_from_trace(
            &self.chat_runtime_config.trace_dir,
            &session_id,
            &thread_id,
            (thread_record.turn_count > 0).then_some(thread_record.turn_count),
        )
        .await
        .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        if recovered.turn_count > 0 {
            thread.write().await.hydrate_from_persisted_state(
                recovered.messages,
                thread_record.token_count.max(recovered.token_count),
                thread_record.turn_count.max(recovered.turn_count),
                updated_at,
            );
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

    async fn estimate_chat_thread_memory(thread: &Arc<RwLock<argus_agent::Thread>>) -> u64 {
        let guard = thread.read().await;
        let history_bytes = guard
            .history()
            .iter()
            .map(|message| message.content.len() as u64)
            .sum::<u64>();
        let plan_bytes = guard.plan().len() as u64 * 128;
        history_bytes + plan_bytes + u64::from(guard.token_count())
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
        let job_id = WorkflowId::new(request.job_id.clone());
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

            if let Err(err) = Self::persist_existing_job_binding(persistence, &job_id, thread_id)
                .await
            {
                return Err(Self::rollback_thread_record(
                    persistence,
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
            status: WorkflowStatus::Pending,
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
        job_id: &WorkflowId,
        thread_id: ThreadId,
    ) -> Result<(), JobError> {
        persistence
            .job_repository
            .update_thread_id(job_id, &thread_id)
            .await
            .map_err(|err| {
                JobError::ExecutionFailed(format!(
                    "failed to persist job-thread binding: {err}"
                ))
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

    async fn recover_thread_state_from_trace(
        trace_dir: &Path,
        session_id: &SessionId,
        thread_id: &ThreadId,
        turn_count_hint: Option<u32>,
    ) -> Result<RecoveredThreadState, std::io::Error> {
        let turns_dir = trace_dir
            .join(session_id.to_string())
            .join(thread_id.to_string())
            .join("turns");
        let mut turn_numbers = Vec::new();
        if let Some(turn_count) = turn_count_hint {
            turn_numbers.extend(1..=turn_count);
        } else if turns_dir.exists() {
            let mut entries = tokio::fs::read_dir(&turns_dir).await?;
            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
                    continue;
                }
                if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str())
                    && let Ok(number) = stem.parse::<u32>()
                {
                    turn_numbers.push(number);
                }
            }
            turn_numbers.sort_unstable();
        }

        let mut messages = Vec::new();
        let mut token_count = 0;
        for turn_number in &turn_numbers {
            let path = turns_dir.join(format!("{turn_number}.jsonl"));
            let events = read_jsonl_events(&path)
                .await
                .map_err(|error| std::io::Error::other(error.to_string()))?;
            for event in events {
                match event {
                    TurnLogEvent::UserInput { content, .. } => {
                        if !content.trim().is_empty() {
                            messages.push(ChatMessage::user(content));
                        }
                    }
                    TurnLogEvent::LlmResponse {
                        content,
                        reasoning_content,
                        tool_calls,
                        ..
                    } => {
                        if tool_calls.is_empty() {
                            if !content.trim().is_empty()
                                || !reasoning_content
                                    .as_deref()
                                    .unwrap_or("")
                                    .trim()
                                    .is_empty()
                            {
                                messages.push(ChatMessage::assistant_with_reasoning(
                                    content,
                                    reasoning_content,
                                ));
                            }
                        }
                    }
                    TurnLogEvent::TurnEnd { token_usage, .. } => {
                        token_count = token_usage.total_tokens;
                    }
                    _ => {}
                }
            }
        }

        Ok(RecoveredThreadState {
            messages,
            turn_count: turn_numbers.len() as u32,
            token_count,
        })
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
            Arc::new(CompactorManager::with_defaults()),
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
    use std::collections::HashMap;
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::sync::Mutex as StdMutex;

    use argus_agent::CompactorManager;
    use argus_protocol::llm::{LlmProviderId, LlmProviderRepository};
    use argus_protocol::{
        AgentRecord, AgentType, LlmProvider, LlmProviderKind, LlmProviderRecord, ProviderId,
        ProviderResolver, ProviderSecretStatus, SecretString, ThinkingConfig, ThreadEvent,
        ThreadId,
    };
    use argus_repository::traits::{AgentRepository, JobRepository, ThreadRepository};
    use argus_repository::types::{
        AgentId as RepoAgentId, JobId, JobRecord, JobResult, JobType, MessageId, MessageRecord,
        ThreadRecord, WorkflowStatus,
    };
    use argus_repository::{ArgusSqlite, DbError, migrate};
    use argus_template::TemplateManager;
    use argus_tool::ToolManager;
    use async_trait::async_trait;
    use sqlx::SqlitePool;
    use tokio::sync::{broadcast, mpsc};

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

    fn drain_events(rx: &mut broadcast::Receiver<ThreadEvent>) -> Vec<ThreadEvent> {
        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }
        events
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
            Arc::new(CompactorManager::with_defaults()),
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
            Arc::new(CompactorManager::with_defaults()),
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
            Arc::new(CompactorManager::with_defaults()),
            std::env::temp_dir().join("argus-thread-pool-tests"),
            Some(ThreadPoolPersistence::new(
                Arc::new(FailingUpdateThreadIdJobRepository::new(
                    job_id,
                    job_thread_id,
                    agent_id,
                ))
                    as Arc<dyn JobRepository>,
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
            _status: WorkflowStatus,
            _started_at: Option<&str>,
            _finished_at: Option<&str>,
        ) -> Result<(), DbError> {
            unreachable!("update_status should not be called")
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
                status: WorkflowStatus::Pending,
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
            _status: WorkflowStatus,
            _started_at: Option<&str>,
            _finished_at: Option<&str>,
        ) -> Result<(), DbError> {
            unreachable!("update_status should not be called")
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
        assert_eq!(snapshot.active_threads, 1);
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
        let pool = ThreadPool::test_pool();
        let request = super::test_request("job-running-metrics");
        let execution_thread_id = pool
            .enqueue_job(request.clone())
            .await
            .expect("enqueue should succeed");
        let (pipe_tx, mut pipe_rx) = broadcast::channel(32);
        let (control_tx, _control_rx) = mpsc::unbounded_channel();

        let _ = pool
            .execute_job(request, execution_thread_id, pipe_tx, control_tx)
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
    async fn execute_job_panic_cleans_up_to_cooling_and_emits_metrics() {
        let pool = ThreadPool::test_pool();
        let mut request = super::test_request("job-panic-cleanup");
        request.prompt = "__panic_thread_pool_execute_turn__".to_string();
        let execution_thread_id = pool
            .enqueue_job(request.clone())
            .await
            .expect("enqueue should succeed");
        let (pipe_tx, mut pipe_rx) = broadcast::channel(32);
        let (control_tx, _control_rx) = mpsc::unbounded_channel();

        let result = pool
            .execute_job(request, execution_thread_id, pipe_tx, control_tx)
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
