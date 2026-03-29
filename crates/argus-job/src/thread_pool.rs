//! ThreadPool for coordinating background job execution.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex as StdMutex};

use argus_agent::{TurnBuilder, TurnConfig, TurnOutput};
use argus_protocol::llm::{ChatMessage, Role};
use argus_protocol::tool::NamedTool;
use argus_protocol::{
    AgentId, ProviderResolver, ThreadControlEvent, ThreadEvent, ThreadId, ThreadJobResult,
    ThreadMailbox, ThreadPoolSnapshot,
};
use argus_template::TemplateManager;
use argus_tool::ToolManager;
use chrono::Utc;
use tokio::sync::{Mutex, broadcast, mpsc};

use crate::error::JobError;
use crate::types::ThreadPoolJobRequest;

const DEFAULT_MAX_THREADS: u32 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RuntimeState {
    Queued,
    Running,
    Cooling,
}

#[derive(Debug, Clone)]
struct RuntimeEntry {
    thread_id: ThreadId,
    state: RuntimeState,
    estimated_memory_bytes: u64,
    last_active_at: String,
}

#[derive(Debug, Default)]
struct ThreadPoolStore {
    runtimes: HashMap<String, RuntimeEntry>,
    evicted_threads: u64,
    peak_estimated_memory_bytes: u64,
    peak_process_memory_bytes: Option<u64>,
}

/// Coordinates job-thread bindings, runtime state transitions, and metrics.
pub struct ThreadPool {
    template_manager: Arc<TemplateManager>,
    provider_resolver: Arc<dyn ProviderResolver>,
    tool_manager: Arc<ToolManager>,
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
    ) -> Self {
        Self {
            template_manager,
            provider_resolver,
            tool_manager,
            max_threads: DEFAULT_MAX_THREADS,
            store: Arc::new(StdMutex::new(ThreadPoolStore::default())),
        }
    }

    /// Bind a job to a concrete execution thread and mark it queued.
    pub async fn enqueue_job(&self, request: ThreadPoolJobRequest) -> Result<ThreadId, JobError> {
        let thread_id = ThreadId::new();
        let now = Utc::now().to_rfc3339();
        let estimated_memory_bytes = request.prompt.chars().count() as u64;

        let mut store = self.store.lock().expect("thread-pool mutex poisoned");
        store.runtimes.insert(
            request.job_id,
            RuntimeEntry {
                thread_id,
                state: RuntimeState::Queued,
                estimated_memory_bytes,
                last_active_at: now,
            },
        );
        Self::refresh_peaks(&mut store);
        Ok(thread_id)
    }

    /// Return the currently bound thread for a job.
    pub fn get_thread_binding(&self, job_id: &str) -> Option<ThreadId> {
        self.store
            .lock()
            .expect("thread-pool mutex poisoned")
            .runtimes
            .get(job_id)
            .map(|entry| entry.thread_id)
    }

    /// Mark a queued runtime as running.
    pub fn mark_running(&self, job_id: &str) -> Option<ThreadId> {
        self.update_state(job_id, RuntimeState::Running)
    }

    /// Mark a runtime as cooling.
    pub fn mark_cooling(&self, job_id: &str) -> Option<ThreadId> {
        self.update_state(job_id, RuntimeState::Cooling)
    }

    /// Evict a runtime that is currently cooling.
    pub fn evict_if_idle(&self, job_id: &str) -> Option<ThreadId> {
        let mut store = self.store.lock().expect("thread-pool mutex poisoned");
        let entry = store.runtimes.get(job_id)?;
        if entry.state != RuntimeState::Cooling {
            return None;
        }
        let removed = store.runtimes.remove(job_id)?;
        store.evicted_threads += 1;
        Some(removed.thread_id)
    }

    /// Collect a point-in-time metrics snapshot for the pool.
    pub fn collect_metrics(&self) -> ThreadPoolSnapshot {
        let store = self.store.lock().expect("thread-pool mutex poisoned");
        let queued_jobs = store
            .runtimes
            .values()
            .filter(|entry| entry.state == RuntimeState::Queued)
            .count() as u32;
        let running_threads = store
            .runtimes
            .values()
            .filter(|entry| entry.state == RuntimeState::Running)
            .count() as u32;
        let cooling_threads = store
            .runtimes
            .values()
            .filter(|entry| entry.state == RuntimeState::Cooling)
            .count() as u32;
        let estimated_memory_bytes = Self::total_estimated_memory(&store);
        let resident_thread_count = store.runtimes.len() as u32;
        let avg_thread_memory_bytes = if resident_thread_count == 0 {
            0
        } else {
            estimated_memory_bytes / u64::from(resident_thread_count)
        };

        ThreadPoolSnapshot {
            max_threads: self.max_threads,
            active_threads: resident_thread_count,
            queued_jobs,
            running_threads,
            cooling_threads,
            evicted_threads: store.evicted_threads,
            estimated_memory_bytes,
            peak_estimated_memory_bytes: store.peak_estimated_memory_bytes,
            process_memory_bytes: None,
            peak_process_memory_bytes: store.peak_process_memory_bytes,
            resident_thread_count,
            avg_thread_memory_bytes,
            captured_at: Utc::now().to_rfc3339(),
        }
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
        let _ = self.mark_running(&request.job_id);
        let _ = pipe_tx.send(ThreadEvent::ThreadPoolStarted {
            job_id: request.job_id.clone(),
            thread_id: execution_thread_id,
        });

        let result = self
            .execute_turn(
                request.originating_thread_id,
                request.job_id.clone(),
                execution_thread_id,
                request.agent_id,
                request.prompt,
                pipe_tx.clone(),
                control_tx,
            )
            .await;

        let _ = self.mark_cooling(&request.job_id);
        let _ = pipe_tx.send(ThreadEvent::ThreadPoolCooling {
            job_id: request.job_id,
            thread_id: execution_thread_id,
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

    fn update_state(&self, job_id: &str, state: RuntimeState) -> Option<ThreadId> {
        let mut store = self.store.lock().expect("thread-pool mutex poisoned");
        let entry = store.runtimes.get_mut(job_id)?;
        entry.state = state;
        entry.last_active_at = Utc::now().to_rfc3339();
        Some(entry.thread_id)
    }

    fn total_estimated_memory(store: &ThreadPoolStore) -> u64 {
        store
            .runtimes
            .values()
            .map(|entry| entry.estimated_memory_bytes)
            .sum()
    }

    fn refresh_peaks(store: &mut ThreadPoolStore) {
        let estimated = Self::total_estimated_memory(store);
        if estimated > store.peak_estimated_memory_bytes {
            store.peak_estimated_memory_bytes = estimated;
        }
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
    assert_eq!(snapshot.queued_jobs, 1);
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
    use super::ThreadPool;

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
}
