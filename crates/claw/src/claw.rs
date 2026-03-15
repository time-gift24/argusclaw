use std::sync::Arc;
use std::{env, path::Path, path::PathBuf};

use sqlx::SqlitePool;
use tokio::sync::RwLock;

use crate::agents::thread::{ThreadConfig, ThreadEvent, ThreadId};
use crate::agents::{AgentManager, AgentRuntimeId};
use crate::db::llm::{LlmProviderId, LlmProviderRecord};
use crate::db::sqlite::{
    SqliteAgentRepository, SqliteJobRepository, SqliteLlmProviderRepository, connect, connect_path,
    migrate,
};
use crate::error::AgentError;
use crate::job::JobRepository;
use crate::llm::LLMManager;
#[cfg(feature = "dev")]
use crate::llm::LlmEventStream;
use crate::scheduler::{Scheduler, SchedulerConfig};
use crate::tool::ToolManager;
use tokio_util::sync::CancellationToken;

/// Result of AppContext initialization with default session info.
#[derive(Clone)]
pub struct AppContextInit {
    /// The initialized AppContext.
    pub context: AppContext,
    /// The default ArgusAgent runtime ID.
    pub agent_runtime_id: AgentRuntimeId,
    /// The default thread ID.
    pub thread_id: ThreadId,
}

/// Active thread information.
#[derive(Debug, Clone)]
pub struct ActiveThread {
    /// Thread ID.
    pub thread_id: ThreadId,
    /// Agent runtime ID that owns this thread.
    pub agent_runtime_id: AgentRuntimeId,
}

#[derive(Clone)]
pub struct AppContext {
    db_pool: SqlitePool,
    llm_manager: Arc<LLMManager>,
    agent_manager: Arc<AgentManager>,
    tool_manager: Arc<ToolManager>,
    job_repository: Arc<dyn JobRepository>,
    shutdown: CancellationToken,
    /// Active threads indexed by thread_id.
    active_threads: Arc<RwLock<Vec<ActiveThread>>>,
}

impl AppContext {
    /// Initialize AppContext with default ArgusAgent and thread.
    ///
    /// This is the recommended way to initialize AppContext for desktop/CLI usage.
    /// Returns the context along with default IDs for immediate use.
    pub async fn init_with_defaults(
        database_target: Option<String>,
    ) -> Result<AppContextInit, AgentError> {
        let context = Self::init(database_target).await?;

        // Initialize ArgusAgent
        let agent_runtime_id = context.init_argus_agent().await?;
        tracing::info!(
            "ArgusAgent initialized with runtime_id: {}",
            agent_runtime_id
        );

        // Create default thread with a deterministic ID
        // Using nil UUID + 1 for the default thread
        let thread_id = ThreadId::parse("00000000-0000-0000-0000-000000000001")
            .expect("default thread ID should be valid UUID");
        context.get_or_create_thread(agent_runtime_id, thread_id, None)?;
        tracing::info!("Default thread created with id: {}", thread_id);

        // Track as active thread
        context
            .add_active_thread(ActiveThread {
                thread_id,
                agent_runtime_id,
            })
            .await;

        Ok(AppContextInit {
            context,
            agent_runtime_id,
            thread_id,
        })
    }

    pub async fn init(database_target: Option<String>) -> Result<Self, AgentError> {
        let database_target = resolve_database_target(database_target)?;
        let pool = match &database_target {
            DatabaseTarget::Url(database_url) => connect(database_url).await,
            DatabaseTarget::Path(path) => {
                ensure_parent_dir(path)?;
                connect_path(path).await
            }
        }?;
        migrate(&pool).await?;

        let llm_repository = Arc::new(SqliteLlmProviderRepository::new(pool.clone()));
        let agent_repository = Arc::new(SqliteAgentRepository::new(pool.clone()));
        let job_repository: Arc<dyn JobRepository> =
            Arc::new(SqliteJobRepository::new(pool.clone()));

        let llm_manager = Arc::new(LLMManager::new(llm_repository));
        let tool_manager = Arc::new(ToolManager::new());
        let agent_manager = Arc::new(AgentManager::new(
            agent_repository,
            llm_manager.clone(),
            tool_manager.clone(),
            None,
        ));

        // Create and start scheduler
        let scheduler = Arc::new(Scheduler::new(
            SchedulerConfig::default(),
            job_repository.clone(),
            agent_manager.clone(),
        ));
        let shutdown = scheduler.shutdown_token();

        // Spawn scheduler as background task
        tokio::spawn({
            let scheduler = scheduler.clone();
            async move {
                scheduler.run().await;
            }
        });

        Ok(Self {
            db_pool: pool,
            llm_manager,
            agent_manager,
            tool_manager,
            job_repository,
            shutdown,
            active_threads: Arc::new(RwLock::new(Vec::new())),
        })
    }

    #[must_use]
    pub fn new(
        llm_manager: Arc<LLMManager>,
        agent_manager: Arc<AgentManager>,
        tool_manager: Arc<ToolManager>,
    ) -> Self {
        Self {
            db_pool: SqlitePool::connect_lazy_with(
                sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(std::path::Path::new(":memory:")),
            ),
            llm_manager,
            agent_manager,
            tool_manager,
            job_repository: Arc::new(SqliteJobRepository::new(SqlitePool::connect_lazy_with(
                sqlx::sqlite::SqliteConnectOptions::new()
                    .filename(std::path::Path::new(":memory:")),
            ))),
            shutdown: CancellationToken::new(),
            active_threads: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Create a new AppContext with an explicit database pool.
    #[must_use]
    pub fn with_pool(
        pool: SqlitePool,
        llm_manager: Arc<LLMManager>,
        agent_manager: Arc<AgentManager>,
        tool_manager: Arc<ToolManager>,
    ) -> Self {
        Self {
            db_pool: pool.clone(),
            llm_manager,
            agent_manager,
            tool_manager,
            job_repository: Arc::new(SqliteJobRepository::new(pool)),
            shutdown: CancellationToken::new(),
            active_threads: Arc::new(RwLock::new(Vec::new())),
        }
    }

    #[must_use]
    pub fn db_pool(&self) -> &SqlitePool {
        &self.db_pool
    }

    #[must_use]
    pub fn llm_manager(&self) -> Arc<LLMManager> {
        Arc::clone(&self.llm_manager)
    }

    #[must_use]
    pub fn agent_manager(&self) -> Arc<AgentManager> {
        Arc::clone(&self.agent_manager)
    }

    #[must_use]
    pub fn tool_manager(&self) -> Arc<ToolManager> {
        Arc::clone(&self.tool_manager)
    }

    #[must_use]
    pub fn job_repository(&self) -> Arc<dyn JobRepository> {
        Arc::clone(&self.job_repository)
    }

    /// Trigger graceful shutdown of the scheduler.
    pub fn shutdown(&self) {
        self.shutdown.cancel();
    }

    pub async fn upsert_provider(&self, record: LlmProviderRecord) -> Result<(), AgentError> {
        self.llm_manager.upsert_provider(record).await
    }

    pub async fn import_providers(
        &self,
        records: Vec<LlmProviderRecord>,
    ) -> Result<(), AgentError> {
        self.llm_manager.import_providers(records).await
    }

    pub async fn get_provider_record(
        &self,
        id: &LlmProviderId,
    ) -> Result<LlmProviderRecord, AgentError> {
        self.llm_manager.get_provider_record(id).await
    }

    pub async fn get_default_provider_record(&self) -> Result<LlmProviderRecord, AgentError> {
        self.llm_manager.get_default_provider_record().await
    }

    pub async fn set_default_provider(&self, id: &LlmProviderId) -> Result<(), AgentError> {
        self.llm_manager.set_default_provider(id).await
    }

    #[cfg(feature = "dev")]
    pub async fn complete_text(
        &self,
        provider_id: Option<&LlmProviderId>,
        prompt: impl Into<String>,
    ) -> Result<String, AgentError> {
        self.llm_manager.complete_text(provider_id, prompt).await
    }

    #[cfg(feature = "dev")]
    pub async fn stream_text(
        &self,
        provider_id: Option<&LlmProviderId>,
        prompt: impl Into<String>,
    ) -> Result<LlmEventStream, AgentError> {
        self.llm_manager.stream_text(provider_id, prompt).await
    }

    // === ArgusAgent API ===

    /// Initialize the ArgusAgent and return its RuntimeId.
    ///
    /// The ArgusAgent is the default assistant for ArgusClaw.
    /// If the agent record has no provider_id, the default provider is used.
    pub async fn init_argus_agent(&self) -> Result<AgentRuntimeId, AgentError> {
        self.agent_manager.init_argus_agent().await
    }

    /// Get the ArgusAgent's RuntimeId if initialized.
    #[must_use]
    pub fn argus_agent_id(&self) -> Option<AgentRuntimeId> {
        self.agent_manager.argus_agent_id()
    }

    /// Get or create a thread for the agent.
    ///
    /// Returns a broadcast receiver for thread events.
    pub fn get_or_create_thread(
        &self,
        agent_runtime_id: AgentRuntimeId,
        thread_id: ThreadId,
        config: Option<ThreadConfig>,
    ) -> Result<tokio::sync::broadcast::Receiver<ThreadEvent>, AgentError> {
        self.agent_manager.get_or_create_thread(
            agent_runtime_id,
            thread_id,
            config.unwrap_or_default(),
        )
    }

    /// Switch the LLM provider for a thread.
    pub async fn switch_thread_provider(
        &self,
        agent_runtime_id: AgentRuntimeId,
        thread_id: ThreadId,
        provider_id: LlmProviderId,
    ) -> Result<(), AgentError> {
        let provider = self.llm_manager.get_provider(&provider_id).await?;
        self.agent_manager
            .switch_thread_provider(agent_runtime_id, thread_id, provider)
    }

    /// Send a message to a thread (non-blocking).
    ///
    /// The response comes through the event stream.
    /// Use `subscribe_thread` to receive events.
    pub async fn send_message(
        &self,
        agent_runtime_id: AgentRuntimeId,
        thread_id: ThreadId,
        message: String,
    ) -> Result<(), AgentError> {
        self.agent_manager
            .send_message(agent_runtime_id, thread_id, message)
            .await
    }

    /// Get the message history for a thread.
    pub fn get_thread_messages(
        &self,
        agent_runtime_id: AgentRuntimeId,
        thread_id: ThreadId,
    ) -> Result<Vec<crate::llm::ChatMessage>, AgentError> {
        self.agent_manager
            .get_thread_messages(agent_runtime_id, thread_id)
    }

    // === Active Thread Management ===

    /// Add a thread to the active threads list.
    pub async fn add_active_thread(&self, thread: ActiveThread) {
        let mut threads = self.active_threads.write().await;
        // Check if already exists
        if !threads.iter().any(|t| t.thread_id == thread.thread_id) {
            threads.push(thread);
        }
    }

    /// Remove a thread from the active threads list.
    pub async fn remove_active_thread(&self, thread_id: ThreadId) {
        let mut threads = self.active_threads.write().await;
        threads.retain(|t| t.thread_id != thread_id);
    }

    /// Get all active threads.
    pub async fn get_active_threads(&self) -> Vec<ActiveThread> {
        self.active_threads.read().await.clone()
    }

    /// Get the default (first) active thread if any.
    pub async fn get_default_thread(&self) -> Option<ActiveThread> {
        self.active_threads.read().await.first().cloned()
    }

    /// Subscribe to a thread's events.
    ///
    /// If the thread doesn't exist, it will be created.
    pub fn subscribe_thread(
        &self,
        agent_runtime_id: AgentRuntimeId,
        thread_id: ThreadId,
    ) -> Result<tokio::sync::broadcast::Receiver<ThreadEvent>, AgentError> {
        self.agent_manager.subscribe(agent_runtime_id, thread_id)
    }
}

enum DatabaseTarget {
    Url(String),
    Path(PathBuf),
}

fn resolve_database_target(configured: Option<String>) -> Result<DatabaseTarget, AgentError> {
    let configured = configured.unwrap_or_else(default_database_target);

    if configured.starts_with("sqlite:") {
        return Ok(DatabaseTarget::Url(configured));
    }

    Ok(DatabaseTarget::Path(expand_home_path(&configured)?))
}

fn default_database_target() -> String {
    env::var("DATABASE_URL").unwrap_or_else(|_| "~/.argusclaw/sqlite.db".to_string())
}

fn expand_home_path(path: &str) -> Result<PathBuf, AgentError> {
    if let Some(relative_path) = path.strip_prefix("~/") {
        let home_dir = dirs::home_dir().ok_or(AgentError::HomeDirectoryUnavailable)?;
        return Ok(home_dir.join(relative_path));
    }

    Ok(PathBuf::from(path))
}

fn ensure_parent_dir(path: &Path) -> Result<(), AgentError> {
    let parent = path
        .parent()
        .ok_or_else(|| AgentError::InvalidDatabasePath {
            path: path.display().to_string(),
        })?;
    std::fs::create_dir_all(parent).map_err(|e| AgentError::DatabaseDirectoryCreateFailed {
        path: parent.display().to_string(),
        reason: e.to_string(),
    })?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use async_trait::async_trait;
    use rust_decimal::Decimal;
    use tempfile::tempdir;
    use tokio::sync::broadcast;
    use tokio::time::timeout;

    use super::{ActiveThread, AppContext, expand_home_path, resolve_database_target};
    use crate::agents::agent::AgentBuilder;
    use crate::agents::thread::{ThreadEvent, ThreadId};
    use crate::agents::{AgentId, AgentManager, AgentRecord, AgentRepository, AgentRuntimeId};
    use crate::db::DbError;
    use crate::db::llm::{LlmProviderId, LlmProviderRecord, LlmProviderRepository};
    use crate::llm::{
        ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LLMManager, LlmError,
        LlmProvider, ToolCompletionRequest, ToolCompletionResponse,
    };
    use crate::tool::ToolManager;

    const DEFAULT_THREAD_ID: &str = "00000000-0000-0000-0000-000000000001";

    struct NoopAgentRepository;

    #[async_trait]
    impl AgentRepository for NoopAgentRepository {
        async fn upsert(&self, _record: &AgentRecord) -> Result<(), DbError> {
            Ok(())
        }

        async fn get(&self, _id: &crate::agents::AgentId) -> Result<Option<AgentRecord>, DbError> {
            Ok(None)
        }

        async fn list(&self) -> Result<Vec<crate::agents::AgentSummary>, DbError> {
            Ok(Vec::new())
        }

        async fn delete(&self, _id: &crate::agents::AgentId) -> Result<bool, DbError> {
            Ok(false)
        }
    }

    struct NoopLlmRepository;

    #[async_trait]
    impl LlmProviderRepository for NoopLlmRepository {
        async fn upsert_provider(&self, _record: &LlmProviderRecord) -> Result<(), DbError> {
            Ok(())
        }

        async fn set_default_provider(&self, _id: &LlmProviderId) -> Result<(), DbError> {
            Ok(())
        }

        async fn get_provider(
            &self,
            _id: &LlmProviderId,
        ) -> Result<Option<LlmProviderRecord>, DbError> {
            Ok(None)
        }

        async fn list_providers(&self) -> Result<Vec<LlmProviderRecord>, DbError> {
            Ok(Vec::new())
        }

        async fn get_default_provider(&self) -> Result<Option<LlmProviderRecord>, DbError> {
            Ok(None)
        }
    }

    #[derive(Default)]
    struct RecordingSequentialMockProvider {
        responses: Mutex<Vec<ToolCompletionResponse>>,
        call_count: Mutex<usize>,
        seen_messages: Mutex<Vec<Vec<ChatMessage>>>,
    }

    impl RecordingSequentialMockProvider {
        fn new(responses: Vec<ToolCompletionResponse>) -> Self {
            Self {
                responses: Mutex::new(responses),
                call_count: Mutex::new(0),
                seen_messages: Mutex::new(Vec::new()),
            }
        }

        fn recorded_messages(&self) -> Vec<Vec<ChatMessage>> {
            self.seen_messages
                .lock()
                .expect("messages mutex should not be poisoned")
                .clone()
        }
    }

    #[async_trait]
    impl LlmProvider for RecordingSequentialMockProvider {
        fn model_name(&self) -> &str {
            "mock"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        fn context_window(&self) -> u32 {
            100_000
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> Result<CompletionResponse, LlmError> {
            unimplemented!("complete is not used in these tests")
        }

        async fn complete_with_tools(
            &self,
            request: ToolCompletionRequest,
        ) -> Result<ToolCompletionResponse, LlmError> {
            self.seen_messages
                .lock()
                .expect("messages mutex should not be poisoned")
                .push(request.messages);

            let mut call_count = self
                .call_count
                .lock()
                .expect("call_count mutex should not be poisoned");
            let responses = self
                .responses
                .lock()
                .expect("responses mutex should not be poisoned");
            let response = responses
                .get(*call_count)
                .cloned()
                .unwrap_or_else(|| panic!("missing response for call {}", *call_count));
            *call_count += 1;
            Ok(response)
        }
    }

    fn build_response(content: &str) -> ToolCompletionResponse {
        ToolCompletionResponse {
            content: Some(content.to_string()),
            reasoning_content: None,
            tool_calls: vec![],
            input_tokens: 16,
            output_tokens: 8,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        }
    }

    fn setup_test_app_context(
        responses: Vec<ToolCompletionResponse>,
    ) -> (
        AppContext,
        AgentRuntimeId,
        Arc<RecordingSequentialMockProvider>,
    ) {
        let provider = Arc::new(RecordingSequentialMockProvider::new(responses));
        let tool_manager = Arc::new(ToolManager::new());
        let llm_manager = Arc::new(LLMManager::new(Arc::new(NoopLlmRepository)));
        let agent_manager = Arc::new(AgentManager::new(
            Arc::new(NoopAgentRepository),
            llm_manager.clone(),
            tool_manager.clone(),
            None,
        ));
        let context = AppContext::new(llm_manager, agent_manager.clone(), tool_manager.clone());

        let agent = AgentBuilder::new()
            .template_id(AgentId::new("argus"))
            .system_prompt(String::new())
            .provider(provider.clone())
            .tool_manager(tool_manager)
            .build();
        let runtime_id = agent.runtime_id();
        agent_manager.agents().insert(runtime_id, Arc::new(agent));

        (context, runtime_id, provider)
    }

    async fn wait_for_turn_completion(
        event_rx: &mut broadcast::Receiver<ThreadEvent>,
    ) -> ThreadEvent {
        loop {
            let event = timeout(Duration::from_secs(2), event_rx.recv())
                .await
                .expect("thread event should arrive before timeout")
                .expect("thread event channel should stay open");

            match event {
                ThreadEvent::TurnCompleted { .. } | ThreadEvent::TurnFailed { .. } => {
                    return event;
                }
                _ => {}
            }
        }
    }

    #[test]
    fn resolve_database_target_keeps_sqlite_urls() {
        let target = resolve_database_target(Some("sqlite::memory:".to_string()))
            .expect("sqlite urls should resolve");

        assert!(matches!(target, super::DatabaseTarget::Url(url) if url == "sqlite::memory:"));
    }

    #[test]
    fn expand_home_path_resolves_tilde_prefix() {
        let path = expand_home_path("~/.argusclaw/sqlite.db").expect("home path should resolve");

        assert!(path.ends_with(".argusclaw/sqlite.db"));
    }

    #[tokio::test]
    async fn init_creates_an_app_context_from_a_filesystem_database_path() {
        let temp_dir = tempdir().expect("temp dir should exist");
        let database_path = temp_dir.path().join("nested").join("sqlite.db");

        let ctx = AppContext::init(Some(database_path.display().to_string()))
            .await
            .expect("app context init should succeed");
        let providers = ctx
            .llm_manager()
            .list_providers()
            .await
            .expect("provider list should succeed");

        assert!(providers.is_empty());
        assert!(database_path.exists());
    }

    #[tokio::test]
    async fn default_thread_send_message_persists_assistant_history() {
        let (context, runtime_id, _provider) =
            setup_test_app_context(vec![build_response("hello from argus")]);
        let thread_id = ThreadId::parse(DEFAULT_THREAD_ID).expect("default thread id should parse");
        let mut event_rx = context
            .get_or_create_thread(runtime_id, thread_id, None)
            .expect("default thread should be created");
        context
            .add_active_thread(ActiveThread {
                thread_id,
                agent_runtime_id: runtime_id,
            })
            .await;

        let default_thread = context
            .get_default_thread()
            .await
            .expect("default thread should be tracked");
        context
            .send_message(
                default_thread.agent_runtime_id,
                default_thread.thread_id,
                "hi".into(),
            )
            .await
            .expect("sending through default thread should succeed");

        let event = wait_for_turn_completion(&mut event_rx).await;
        assert!(matches!(event, ThreadEvent::TurnCompleted { .. }));

        let messages = context
            .get_thread_messages(default_thread.agent_runtime_id, default_thread.thread_id)
            .expect("message history should be readable");
        let contents: Vec<_> = messages
            .iter()
            .map(|message| message.content.as_str())
            .collect();
        assert_eq!(contents, vec!["hi", "hello from argus"]);
    }

    #[tokio::test]
    async fn second_turn_reuses_prior_assistant_context() {
        let (context, runtime_id, provider) = setup_test_app_context(vec![
            build_response("first answer"),
            build_response("second answer"),
        ]);
        let thread_id = ThreadId::parse(DEFAULT_THREAD_ID).expect("default thread id should parse");
        let mut event_rx = context
            .get_or_create_thread(runtime_id, thread_id, None)
            .expect("default thread should be created");
        context
            .send_message(runtime_id, thread_id, "first question".into())
            .await
            .expect("first send should succeed");
        wait_for_turn_completion(&mut event_rx).await;

        context
            .send_message(runtime_id, thread_id, "second question".into())
            .await
            .expect("second send should succeed");
        wait_for_turn_completion(&mut event_rx).await;

        let recorded = provider.recorded_messages();
        assert_eq!(recorded.len(), 2);

        let first_roles: Vec<_> = recorded[0].iter().map(|message| message.role).collect();
        let second_contents: Vec<_> = recorded[1]
            .iter()
            .map(|message| message.content.as_str())
            .collect();

        assert_eq!(first_roles, vec![crate::llm::Role::User]);
        assert_eq!(
            second_contents,
            vec!["first question", "first answer", "second question"]
        );
    }

    #[tokio::test]
    async fn custom_thread_id_is_preserved_across_events_and_history() {
        let (context, runtime_id, _provider) =
            setup_test_app_context(vec![build_response("custom thread answer")]);
        let custom_thread_id = ThreadId::parse("00000000-0000-0000-0000-000000000099")
            .expect("custom thread id should parse");
        let mut event_rx = context
            .get_or_create_thread(runtime_id, custom_thread_id, None)
            .expect("custom thread should be created");

        context
            .send_message(runtime_id, custom_thread_id, "custom hi".into())
            .await
            .expect("custom thread send should succeed");

        let event = wait_for_turn_completion(&mut event_rx).await;
        match event {
            ThreadEvent::TurnCompleted { thread_id, .. } => assert_eq!(thread_id, custom_thread_id),
            other => panic!("expected TurnCompleted event, got {:?}", other),
        }

        let messages = context
            .get_thread_messages(runtime_id, custom_thread_id)
            .expect("custom thread history should exist");
        let contents: Vec<_> = messages
            .iter()
            .map(|message| message.content.as_str())
            .collect();
        assert_eq!(contents, vec!["custom hi", "custom thread answer"]);
    }
}
