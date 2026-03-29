//! ThreadPool for coordinating background job execution.

use std::collections::{HashMap, HashSet};
use std::panic::AssertUnwindSafe;
use std::sync::{Arc, Mutex as StdMutex};

use argus_agent::{TurnBuilder, TurnConfig, TurnOutput};
use argus_protocol::llm::{ChatMessage, Role};
use argus_protocol::tool::NamedTool;
use argus_protocol::{
    AgentId, ProviderResolver, ThreadControlEvent, ThreadEvent, ThreadId, ThreadJobResult,
    ThreadMailbox, ThreadPoolSnapshot,
};
use argus_repository::traits::{JobRepository, LlmProviderRepository, ThreadRepository};
use argus_repository::types::{
    AgentId as RepoAgentId, JobRecord, JobType, ThreadRecord, WorkflowId, WorkflowStatus,
};
use argus_template::TemplateManager;
use argus_tool::ToolManager;
use chrono::Utc;
use futures_util::FutureExt;
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
    ) -> Self {
        Self::with_persistence(template_manager, provider_resolver, tool_manager, None)
    }

    /// Create a thread pool with optional repository-backed persistence.
    pub fn with_persistence(
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        tool_manager: Arc<ToolManager>,
        persistence: Option<ThreadPoolPersistence>,
    ) -> Self {
        Self {
            template_manager,
            provider_resolver,
            tool_manager,
            persistence,
            max_threads: DEFAULT_MAX_THREADS,
            store: Arc::new(StdMutex::new(ThreadPoolStore::default())),
        }
    }

    /// Bind a job to a concrete execution thread and mark it queued.
    pub async fn enqueue_job(&self, request: ThreadPoolJobRequest) -> Result<ThreadId, JobError> {
        let now = Utc::now().to_rfc3339();
        let thread_id = self.persist_binding(&request, &now).await?;
        let estimated_memory_bytes = request.prompt.len() as u64;

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
        let fallback_job_id = request.job_id.clone();
        let fallback_agent_id = request.agent_id;
        let fallback_display_name = format!("Agent {}", fallback_agent_id.inner());

        let _ = self.mark_running(&request.job_id);
        let _ = pipe_tx.send(ThreadEvent::ThreadPoolStarted {
            job_id: request.job_id.clone(),
            thread_id: execution_thread_id,
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
            job_id: request.job_id.clone(),
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

    fn panic_message(payload: Box<dyn std::any::Any + Send>) -> String {
        let payload = payload.as_ref();
        let detail = payload
            .downcast_ref::<&'static str>()
            .map(|msg| (*msg).to_string())
            .or_else(|| payload.downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "unknown panic payload".to_string());
        format!("job executor panicked: {detail}")
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
        let thread_id = existing_job
            .as_ref()
            .and_then(|job| job.thread_id)
            .unwrap_or_else(ThreadId::new);

        let thread_record = ThreadRecord {
            id: thread_id,
            provider_id,
            title: Some(format!("job:{}", request.job_id)),
            token_count: 0,
            turn_count: 0,
            session_id: None,
            template_id: Some(RepoAgentId::new(request.agent_id.inner())),
            model_override,
            created_at: now.to_string(),
            updated_at: now.to_string(),
        };
        persistence
            .thread_repository
            .upsert_thread(&thread_record)
            .await
            .map_err(|err| {
                JobError::ExecutionFailed(format!("failed to persist thread record: {err}"))
            })?;

        if existing_job.is_some() {
            Self::persist_existing_job_binding(persistence, &job_id, thread_id).await?;
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
        if let Err(err) = persistence
            .job_repository
            .update_thread_id(job_id, &thread_id)
            .await
        {
            return Err(Self::rollback_thread_record(
                persistence,
                thread_id,
                format!("failed to persist job-thread binding: {err}"),
            )
            .await);
        }

        Ok(())
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
    use std::collections::HashMap;
    use std::collections::HashSet;
    use std::sync::Arc;
    use std::sync::Mutex as StdMutex;

    use argus_protocol::llm::{LlmProviderId, LlmProviderRepository};
    use argus_protocol::{
        AgentRecord, AgentType, LlmProvider, LlmProviderKind, LlmProviderRecord, ProviderId,
        ProviderResolver, ProviderSecretStatus, SecretString, ThinkingConfig, ThreadEvent,
    };
    use argus_repository::traits::{AgentRepository, JobRepository, ThreadRepository};
    use argus_repository::types::{
        JobId, JobRecord, JobResult, MessageId, MessageRecord, ThreadRecord, WorkflowStatus,
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
            Some(ThreadPoolPersistence::new(
                Arc::new(FailingCreateJobRepository) as Arc<dyn JobRepository>,
                thread_repository.clone() as Arc<dyn ThreadRepository>,
                sqlite as Arc<dyn LlmProviderRepository>,
            )),
        );

        (thread_pool, thread_repository)
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
        assert_eq!(snapshot.queued_jobs, 1);
        assert_eq!(snapshot.evicted_threads, 1);
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
                        && snapshot.queued_jobs == 0
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
