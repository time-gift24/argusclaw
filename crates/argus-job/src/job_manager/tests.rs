use std::sync::Arc;

use argus_agent::thread_trace_store::{
    ThreadTraceKind, ThreadTraceMetadata, chat_thread_base_dir, persist_thread_metadata,
};
use argus_protocol::llm::{
    CompletionRequest, CompletionResponse, FinishReason, LlmError, LlmProviderRepository,
};
use argus_protocol::{
    AgentRecord, LlmProvider, ProviderId, SessionId, ThinkingConfig, ThreadId, ThreadRuntimeStatus,
};
use argus_repository::ArgusSqlite;
use argus_repository::migrate;
use argus_repository::traits::{
    AgentRepository, JobRepository, SessionRepository, ThreadRepository,
};
use argus_repository::types::{AgentId as RepoAgentId, ThreadRecord};
use argus_template::TemplateManager;
use async_trait::async_trait;
use rust_decimal::Decimal;
use sqlx::SqlitePool;

use argus_protocol::TokenUsage;
use argus_tool::ToolManager;
use tokio::time::{Duration, sleep, timeout};

use super::*;

mod cancellation;
mod execution;
mod recovery;
mod summary;
mod tracking;

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
    let thread_pool = Arc::new(ThreadPool::new());
    JobManager::new(
        thread_pool,
        Arc::new(TemplateManager::new(
            sqlite.clone() as Arc<dyn AgentRepository>,
            sqlite.clone(),
        )),
        Arc::new(DummyProviderResolver),
        Arc::new(ToolManager::new()),
        std::env::temp_dir().join("argus-job-tests"),
    )
}

struct FixedProviderResolver {
    provider: Arc<dyn LlmProvider>,
}

impl FixedProviderResolver {
    fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
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

async fn test_job_manager_with_provider(
    provider: Arc<dyn LlmProvider>,
) -> (JobManager, AgentId, ThreadId) {
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
        display_name: "Cancellable Job Agent".to_string(),
        description: "Used to test stop_job cancellation".to_string(),
        version: "1.0.0".to_string(),
        provider_id: Some(ProviderId::new(1)),
        model_id: Some("capturing".to_string()),
        system_prompt: "You are a cancellable test agent.".to_string(),
        tool_names: vec![],
        subagent_names: vec![],
        max_tokens: None,
        temperature: None,
        thinking_config: Some(ThinkingConfig::enabled()),
    };
    template_manager
        .upsert(agent_record.clone())
        .await
        .expect("agent upsert should succeed");

    let trace_dir = std::env::temp_dir().join(format!("argus-job-tests-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&trace_dir).expect("trace dir should exist");
    let parent_session_id = SessionId::new();
    let parent_thread_id = ThreadId::new();
    SessionRepository::create(sqlite.as_ref(), &parent_session_id, "job-parent")
        .await
        .expect("parent session should persist");
    ThreadRepository::upsert_thread(
        sqlite.as_ref(),
        &ThreadRecord {
            id: parent_thread_id,
            provider_id: argus_protocol::LlmProviderId::new(1),
            title: Some("job-parent".to_string()),
            token_count: 0,
            turn_count: 0,
            session_id: Some(parent_session_id),
            template_id: Some(RepoAgentId::new(agent_id.inner())),
            model_override: Some("capturing".to_string()),
            created_at: chrono::Utc::now().to_rfc3339(),
            updated_at: chrono::Utc::now().to_rfc3339(),
        },
    )
    .await
    .expect("parent thread should persist");
    persist_thread_metadata(
        &chat_thread_base_dir(&trace_dir, parent_session_id, parent_thread_id),
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
    .expect("parent trace metadata should persist");

    (
        JobManager::new_with_repositories(
            Arc::new(ThreadPool::new()),
            template_manager,
            Arc::new(FixedProviderResolver::new(provider)),
            Arc::new(ToolManager::new()),
            trace_dir,
            Some(sqlite.clone() as Arc<dyn JobRepository>),
            Some(sqlite.clone() as Arc<dyn ThreadRepository>),
            Some(sqlite as Arc<dyn LlmProviderRepository>),
        ),
        agent_id,
        parent_thread_id,
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
        Arc::new(ThreadPool::new()),
        Arc::new(TemplateManager::new(
            sqlite.clone() as Arc<dyn AgentRepository>,
            sqlite.clone(),
        )),
        Arc::new(DummyProviderResolver),
        Arc::new(ToolManager::new()),
        std::env::temp_dir().join("argus-job-tests"),
        Some(sqlite.clone() as Arc<dyn JobRepository>),
        Some(sqlite.clone() as Arc<dyn ThreadRepository>),
        Some(sqlite as Arc<dyn LlmProviderRepository>),
    )
}

fn assistant_output(content: &str) -> TurnRecord {
    TurnRecord::user_turn(
        1,
        vec![ChatMessage::assistant(content)],
        TokenUsage::default(),
    )
}

fn sample_job_result(job_id: impl Into<String>) -> ThreadJobResult {
    ThreadJobResult {
        job_id: job_id.into(),
        success: true,
        cancelled: false,
        message: "all done".to_string(),
        token_usage: None,
        agent_id: AgentId::new(9),
        agent_display_name: "Researcher".to_string(),
        agent_description: "Looks things up".to_string(),
    }
}
