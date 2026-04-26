use std::collections::BTreeMap;
use std::sync::Arc;

use argus_auth::AccountManager;
use argus_crypto::{Cipher, FileKeySource};
use argus_job::JobManager;
use argus_llm::ProviderManager;
use argus_mcp::{McpRuntime, McpRuntimeConfig, RmcpConnector};
use argus_protocol::llm::ChatMessage;
use argus_protocol::{
    AgentId, AgentRecord, ArgusError, JobRuntimeState, LlmProviderId, LlmProviderRecord,
    McpDiscoveredToolRecord, McpRuntimeHeaderOverrides, McpRuntimeHeaders, McpServerRecord,
    McpServerStatus, McpToolResolutionContext, McpToolResolver, McpTransportConfig, ProviderId,
    ProviderResolver, ProviderTestResult, ResolvedMcpTools, Result, RiskLevel, SessionId,
    ThreadEvent, ThreadId, ThreadPoolState, ThreadRuntimeStatus,
};
use argus_repository::traits::{
    AccountRepository, AgentRepository, AgentRunRepository, JobRepository, LlmProviderRepository,
    McpRepository, SessionRepository, ThreadRepository,
};
use argus_repository::types::{AgentRunId, AgentRunRecord, AgentRunStatus};
use argus_repository::{ArgusSqlite, connect, connect_path, migrate};
use argus_session::{SessionManager, SessionSummary, ThreadSummary};
use argus_template::TemplateManager;
use argus_thread_pool::ThreadPool;
use argus_tool::ToolManager;
use async_trait::async_trait;
use axum::http::{HeaderName, HeaderValue};
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use thiserror::Error;
use tokio::sync::broadcast;

use crate::agent_run_context::AgentRunContextRegistry;
use crate::db::{DatabaseTarget, default_trace_dir, ensure_parent_dir, resolve_database_target};
use crate::resolver::ProviderManagerResolver;

const DEFAULT_AGENT_DISPLAY_NAME: &str = "ArgusWing";
const DEFAULT_INSTANCE_NAME: &str = "ArgusWing";

pub struct ServerCore {
    provider_manager: Arc<ProviderManager>,
    template_manager: Arc<TemplateManager>,
    session_manager: Arc<SessionManager>,
    tool_manager: Arc<ToolManager>,
    job_manager: Arc<JobManager>,
    mcp_runtime: Arc<McpRuntime>,
    _account_manager: Arc<AccountManager>,
    mcp_repo: Arc<dyn McpRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    agent_run_contexts: AgentRunContextRegistry,
}

struct RunScopedMcpToolResolver {
    inner: Arc<dyn McpToolResolver>,
    registry: AgentRunContextRegistry,
}

impl RunScopedMcpToolResolver {
    fn new(inner: Arc<dyn McpToolResolver>, registry: AgentRunContextRegistry) -> Self {
        Self { inner, registry }
    }
}

#[async_trait]
impl McpToolResolver for RunScopedMcpToolResolver {
    async fn resolve_for_agent(
        &self,
        agent_id: AgentId,
        context: &McpToolResolutionContext,
    ) -> Result<ResolvedMcpTools> {
        let mut scoped_context = context.clone();
        if let Some(thread_id) = context.thread_id {
            let headers = self.registry.headers_for_thread(thread_id);
            if !headers.is_empty() {
                scoped_context.runtime_headers = headers;
            }
        }
        self.inner
            .resolve_for_agent(agent_id, &scoped_context)
            .await
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolRegistryItem {
    pub name: String,
    pub description: String,
    pub risk_level: RiskLevel,
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatThreadSnapshot {
    pub session_id: SessionId,
    pub thread_id: ThreadId,
    pub messages: Vec<ChatMessage>,
    pub turn_count: u32,
    pub token_count: u32,
    pub plan_item_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatThreadBinding {
    pub session_id: SessionId,
    pub thread_id: ThreadId,
    pub template_id: AgentId,
    pub effective_provider_id: Option<ProviderId>,
    pub effective_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChatSessionPayload {
    pub session_key: String,
    pub session_id: SessionId,
    pub template_id: AgentId,
    pub thread_id: ThreadId,
    pub effective_provider_id: Option<ProviderId>,
    pub effective_model: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunSummary {
    pub run_id: AgentRunId,
    pub agent_id: AgentId,
    pub status: AgentRunStatus,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentRunDetail {
    pub run_id: AgentRunId,
    pub agent_id: AgentId,
    pub status: AgentRunStatus,
    pub prompt: String,
    pub result: Option<String>,
    pub error: Option<String>,
    pub created_at: String,
    pub updated_at: String,
    pub completed_at: Option<String>,
}

#[derive(Debug, Error)]
pub enum McpHeaderOverrideError {
    #[error("{0}")]
    BadRequest(String),
    #[error(transparent)]
    Internal(ArgusError),
}

impl From<&AgentRunRecord> for AgentRunSummary {
    fn from(record: &AgentRunRecord) -> Self {
        AgentRunSummary {
            run_id: record.id,
            agent_id: record.agent_id,
            status: record.status,
            created_at: record.created_at.clone(),
            updated_at: record.updated_at.clone(),
        }
    }
}

impl From<&AgentRunRecord> for AgentRunDetail {
    fn from(record: &AgentRunRecord) -> Self {
        AgentRunDetail {
            run_id: record.id,
            agent_id: record.agent_id,
            status: record.status,
            prompt: record.prompt.clone(),
            result: record.result.clone(),
            error: record.error.clone(),
            created_at: record.created_at.clone(),
            updated_at: record.updated_at.clone(),
            completed_at: record.completed_at.clone(),
        }
    }
}

impl ServerCore {
    pub async fn init(database_path: Option<&str>) -> Result<Arc<Self>> {
        let database_target = resolve_database_target(database_path)?;
        let pool = match &database_target {
            DatabaseTarget::Url(database_url) => connect(database_url).await,
            DatabaseTarget::Path(path) => {
                ensure_parent_dir(path)?;
                connect_path(path).await
            }
        }?;
        migrate(&pool).await?;

        Self::from_pool(pool).await
    }

    pub async fn with_pool(pool: SqlitePool) -> Result<Arc<Self>> {
        Self::from_pool(pool).await
    }

    async fn from_pool(pool: SqlitePool) -> Result<Arc<Self>> {
        let cipher = Arc::new(Cipher::new(FileKeySource::from_env_or_default()));
        let account_repo: Arc<dyn AccountRepository> = Arc::new(ArgusSqlite::new(pool.clone()));
        let account_manager = Arc::new(AccountManager::new(account_repo.clone(), cipher.clone()));

        let llm_repository: Arc<dyn LlmProviderRepository> =
            Arc::new(ArgusSqlite::new(pool.clone()));
        let provider_manager =
            Arc::new(ProviderManager::new(llm_repository.clone()).with_auth(account_repo, cipher));

        let sqlite = Arc::new(ArgusSqlite::new(pool));
        let template_manager = Arc::new(TemplateManager::new(
            sqlite.clone() as Arc<dyn AgentRepository>,
            sqlite.clone(),
        ));
        Self::bootstrap_template_manager(Arc::clone(&template_manager)).await?;

        let tool_manager = Arc::new(ToolManager::new());
        Self::register_default_tools(&tool_manager);
        let trace_dir = default_trace_dir();
        let _ = std::fs::create_dir_all(&trace_dir);

        let provider_resolver: Arc<dyn ProviderResolver> =
            Arc::new(ProviderManagerResolver::new(Arc::clone(&provider_manager)));
        let thread_pool = Arc::new(ThreadPool::new());

        let job_manager = Arc::new(JobManager::new_with_repositories(
            Arc::clone(&thread_pool),
            Arc::clone(&template_manager),
            Arc::clone(&provider_resolver),
            Arc::clone(&tool_manager),
            trace_dir.clone(),
            Some(sqlite.clone() as Arc<dyn JobRepository>),
            Some(sqlite.clone() as Arc<dyn ThreadRepository>),
            Some(Arc::clone(&llm_repository)),
        ));

        let mcp_repo: Arc<dyn McpRepository> = sqlite.clone();
        let agent_run_repo: Arc<dyn AgentRunRepository> = sqlite.clone();
        let mcp_runtime = Arc::new(McpRuntime::new(
            Arc::clone(&mcp_repo),
            Arc::new(RmcpConnector),
            McpRuntimeConfig::default(),
        ));
        McpRuntime::start(&mcp_runtime);
        let agent_run_contexts = AgentRunContextRegistry::default();
        let base_mcp_tool_resolver: Arc<dyn McpToolResolver> =
            Arc::new(McpRuntime::handle(&mcp_runtime));
        let mcp_tool_resolver: Arc<dyn McpToolResolver> = Arc::new(RunScopedMcpToolResolver::new(
            base_mcp_tool_resolver,
            agent_run_contexts.clone(),
        ));
        job_manager.set_mcp_tool_resolver(Some(Arc::clone(&mcp_tool_resolver)));
        let child_contexts = agent_run_contexts.clone();
        job_manager.set_job_thread_created_hook(Some(Arc::new(move |parent, child| {
            if let Err(error) = child_contexts.inherit_thread(parent, child) {
                tracing::debug!(%parent, %child, %error, "job thread has no agent run context to inherit");
            }
        })));
        let finished_contexts = agent_run_contexts.clone();
        job_manager.set_job_thread_finished_hook(Some(Arc::new(move |thread_id| {
            finished_contexts.release_thread(thread_id);
        })));

        let session_manager = Arc::new(SessionManager::new(
            sqlite.clone() as Arc<dyn SessionRepository>,
            sqlite.clone() as Arc<dyn ThreadRepository>,
            Arc::clone(&llm_repository),
            Arc::clone(&template_manager),
            provider_resolver,
            mcp_tool_resolver,
            Arc::clone(&tool_manager),
            trace_dir,
            thread_pool,
            Arc::clone(&job_manager),
        ));

        Ok(Arc::new(Self {
            provider_manager,
            template_manager,
            session_manager,
            tool_manager,
            job_manager,
            mcp_runtime,
            _account_manager: account_manager,
            mcp_repo,
            agent_run_repo,
            agent_run_contexts,
        }))
    }

    async fn bootstrap_template_manager(template_manager: Arc<TemplateManager>) -> Result<()> {
        template_manager.repair_placeholder_ids().await?;
        template_manager.seed_builtin_agents().await?;
        Ok(())
    }

    fn register_default_tools(tool_manager: &Arc<ToolManager>) {
        use argus_tool::{
            ApplyPatchTool, ChromeTool, GlobTool, GrepTool, HttpTool, ListDirTool, ReadTool,
            ShellTool, SleepTool, WriteFileTool,
        };

        tool_manager.register(Arc::new(ShellTool::new()));
        tool_manager.register(Arc::new(ReadTool::new()));
        tool_manager.register(Arc::new(GrepTool::new()));
        tool_manager.register(Arc::new(GlobTool::new()));
        tool_manager.register(Arc::new(HttpTool::new()));
        tool_manager.register(Arc::new(WriteFileTool::new()));
        tool_manager.register(Arc::new(ListDirTool::new()));
        tool_manager.register(Arc::new(ApplyPatchTool::new()));
        tool_manager.register(Arc::new(SleepTool::new()));
        tool_manager.register(Arc::new(ChromeTool::new_interactive()));
    }

    pub fn instance_name(&self) -> &'static str {
        DEFAULT_INSTANCE_NAME
    }

    pub async fn list_providers(&self) -> Result<Vec<LlmProviderRecord>> {
        self.provider_manager.list_providers().await
    }

    pub async fn get_provider_record(&self, id: LlmProviderId) -> Result<LlmProviderRecord> {
        self.provider_manager.get_provider_record(&id).await
    }

    pub async fn upsert_provider(&self, record: LlmProviderRecord) -> Result<LlmProviderId> {
        self.provider_manager.upsert_provider(record).await
    }

    pub async fn set_default_provider(&self, id: LlmProviderId) -> Result<()> {
        self.provider_manager.set_default_provider(&id).await
    }

    pub async fn get_default_provider_record(&self) -> Result<LlmProviderRecord> {
        self.provider_manager.get_default_provider_record().await
    }

    pub async fn delete_provider(&self, id: LlmProviderId) -> Result<bool> {
        self.provider_manager.delete_provider(&id).await
    }

    pub async fn test_provider_connection(
        &self,
        id: LlmProviderId,
        model: Option<String>,
    ) -> Result<ProviderTestResult> {
        let model = match model.filter(|value| !value.trim().is_empty()) {
            Some(model) => model,
            None => self.get_provider_record(id).await?.default_model,
        };
        self.provider_manager
            .test_provider_connection(&id, &model)
            .await
    }

    pub async fn test_provider_record(
        &self,
        record: LlmProviderRecord,
        model: Option<String>,
    ) -> Result<ProviderTestResult> {
        let model = model
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| record.default_model.clone());
        self.provider_manager
            .test_provider_record(record, &model)
            .await
    }

    pub async fn list_templates(&self) -> Result<Vec<AgentRecord>> {
        self.template_manager.list().await
    }

    pub async fn get_template(&self, id: AgentId) -> Result<Option<AgentRecord>> {
        self.template_manager.get(id).await
    }

    pub async fn upsert_template(&self, record: AgentRecord) -> Result<AgentId> {
        self.template_manager.upsert(record).await
    }

    pub async fn delete_template(&self, id: AgentId) -> Result<()> {
        self.template_manager.delete(id).await
    }

    pub async fn get_default_template(&self) -> Result<Option<AgentRecord>> {
        self.template_manager
            .find_by_display_name(DEFAULT_AGENT_DISPLAY_NAME)
            .await
            .map_err(|error| ArgusError::DatabaseError {
                reason: format!("Failed to fetch default template: {error}"),
            })
    }

    pub async fn list_mcp_servers(&self) -> Result<Vec<McpServerRecord>> {
        self.mcp_repo
            .list_mcp_servers()
            .await
            .map_err(database_error)
    }

    pub async fn get_mcp_server(&self, id: i64) -> Result<Option<McpServerRecord>> {
        self.mcp_repo
            .get_mcp_server(id)
            .await
            .map_err(database_error)
    }

    pub async fn upsert_mcp_server(&self, record: McpServerRecord) -> Result<i64> {
        self.mcp_repo
            .upsert_mcp_server(&record)
            .await
            .map_err(database_error)
    }

    pub async fn delete_mcp_server(&self, id: i64) -> Result<bool> {
        let deleted = self
            .mcp_repo
            .delete_mcp_server(id)
            .await
            .map_err(database_error)?;
        if deleted {
            self.mcp_runtime
                .poll_once()
                .await
                .map_err(ArgusError::from)?;
        }
        Ok(deleted)
    }

    pub async fn test_mcp_server_input(
        &self,
        record: McpServerRecord,
    ) -> Result<argus_mcp::McpConnectionTestResult> {
        self.mcp_runtime
            .test_server_input(record)
            .await
            .map_err(ArgusError::from)
    }

    pub async fn test_mcp_server_connection(
        &self,
        id: i64,
    ) -> Result<argus_mcp::McpConnectionTestResult> {
        let record = self
            .get_mcp_server(id)
            .await?
            .ok_or_else(|| ArgusError::DatabaseError {
                reason: format!("MCP server not found: {id}"),
            })?;
        let result = self.test_mcp_server_input(record.clone()).await?;
        let mut persisted_record = record;
        persisted_record.last_checked_at = Some(result.checked_at.clone());
        persisted_record.last_error = Some(result.message.clone());
        persisted_record.discovered_tool_count = result.discovered_tools.len() as u32;

        if persisted_record.enabled && result.status == McpServerStatus::Ready {
            persisted_record.status = McpServerStatus::Ready;
            persisted_record.last_success_at = Some(result.checked_at.clone());
            persisted_record.last_error = None;
            self.mcp_repo
                .replace_mcp_server_tools(id, &result.discovered_tools)
                .await
                .map_err(database_error)?;
            self.mcp_repo
                .upsert_mcp_server(&persisted_record)
                .await
                .map_err(database_error)?;
            self.mcp_runtime
                .poll_once()
                .await
                .map_err(ArgusError::from)?;
        } else {
            persisted_record.status = if persisted_record.enabled {
                result.status
            } else {
                McpServerStatus::Disabled
            };
            self.mcp_repo
                .upsert_mcp_server(&persisted_record)
                .await
                .map_err(database_error)?;
        }

        Ok(result)
    }

    pub async fn list_mcp_server_tools(
        &self,
        server_id: i64,
    ) -> Result<Vec<McpDiscoveredToolRecord>> {
        self.mcp_repo
            .list_mcp_server_tools(server_id)
            .await
            .map_err(database_error)
    }

    pub fn list_tools(&self) -> Vec<ToolRegistryItem> {
        let mut tools = self
            .tool_manager
            .list_definitions()
            .into_iter()
            .map(|definition| ToolRegistryItem {
                risk_level: self.tool_manager.get_risk_level(&definition.name),
                name: definition.name,
                description: definition.description,
                parameters: definition.parameters,
            })
            .collect::<Vec<_>>();
        tools.sort_by(|left, right| left.name.cmp(&right.name));
        tools
    }

    pub fn thread_pool_state(&self) -> ThreadPoolState {
        self.session_manager.thread_pool_state()
    }

    pub fn job_runtime_state(&self) -> JobRuntimeState {
        self.job_manager.job_runtime_state()
    }

    pub async fn list_chat_sessions(&self) -> Result<Vec<SessionSummary>> {
        self.session_manager.list_sessions().await
    }

    pub async fn create_chat_session(&self, name: String) -> Result<SessionSummary> {
        let session_id = self.session_manager.create(name).await?;
        self.list_chat_sessions()
            .await?
            .into_iter()
            .find(|session| session.id == session_id)
            .ok_or_else(|| missing_after_mutation("session", session_id))
    }

    pub async fn delete_chat_session(&self, session_id: SessionId) -> Result<()> {
        self.session_manager.delete(session_id).await
    }

    pub async fn rename_chat_session(
        &self,
        session_id: SessionId,
        name: String,
    ) -> Result<SessionSummary> {
        self.session_manager
            .rename_session(session_id, name)
            .await?;
        self.list_chat_sessions()
            .await?
            .into_iter()
            .find(|session| session.id == session_id)
            .ok_or_else(|| missing_after_mutation("session", session_id))
    }

    pub async fn list_chat_threads(&self, session_id: SessionId) -> Result<Vec<ThreadSummary>> {
        self.session_manager.list_threads(session_id).await
    }

    pub async fn create_chat_thread(
        &self,
        session_id: SessionId,
        template_id: AgentId,
        provider_id: Option<ProviderId>,
        model: Option<String>,
    ) -> Result<ThreadSummary> {
        let thread_id = self
            .session_manager
            .create_thread(session_id, template_id, provider_id, model.as_deref())
            .await?;
        self.list_chat_threads(session_id)
            .await?
            .into_iter()
            .find(|thread| thread.id == thread_id)
            .ok_or_else(|| missing_after_mutation("thread", thread_id))
    }

    pub async fn create_chat_session_with_thread(
        &self,
        session_name: String,
        template_id: AgentId,
        provider_id: Option<ProviderId>,
        model: Option<String>,
    ) -> Result<ChatSessionPayload> {
        let session_id = self.session_manager.create(session_name).await?;
        let thread_id = match self
            .session_manager
            .create_thread(session_id, template_id, provider_id, model.as_deref())
            .await
        {
            Ok(thread_id) => thread_id,
            Err(error) => {
                self.cleanup_failed_chat_session(session_id).await;
                return Err(error);
            }
        };
        let binding = match self.activate_chat_thread(session_id, thread_id).await {
            Ok(binding) => binding,
            Err(error) => {
                self.cleanup_failed_chat_session(session_id).await;
                return Err(error);
            }
        };
        let provider_key = provider_id
            .map(|id| id.inner().to_string())
            .unwrap_or_else(|| "__default__".to_string());

        Ok(ChatSessionPayload {
            session_key: format!("{}::{}", template_id.inner(), provider_key),
            session_id,
            template_id: binding.template_id,
            thread_id,
            effective_provider_id: binding.effective_provider_id,
            effective_model: binding.effective_model,
        })
    }

    async fn cleanup_failed_chat_session(&self, session_id: SessionId) {
        if let Err(error) = self.session_manager.delete(session_id).await {
            tracing::warn!(
                session_id = %session_id,
                error = %error,
                "failed to clean up chat session after materialization error"
            );
        }
    }

    async fn cleanup_failed_agent_run_session(&self, session_id: SessionId, thread_id: ThreadId) {
        if let Err(error) = self
            .session_manager
            .delete_thread(session_id, &thread_id)
            .await
        {
            tracing::warn!(
                session_id = %session_id,
                thread_id = %thread_id,
                error = %error,
                "failed to delete agent run thread after materialization error"
            );
        }
        self.cleanup_failed_chat_session(session_id).await;
    }

    pub async fn delete_chat_thread(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Result<()> {
        self.session_manager
            .delete_thread(session_id, &thread_id)
            .await
    }

    pub async fn rename_chat_thread(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
        title: String,
    ) -> Result<ThreadSummary> {
        self.session_manager
            .rename_thread(session_id, &thread_id, title)
            .await?;
        self.list_chat_threads(session_id)
            .await?
            .into_iter()
            .find(|thread| thread.id == thread_id)
            .ok_or_else(|| missing_after_mutation("thread", thread_id))
    }

    pub async fn update_chat_thread_model(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
        provider_id: ProviderId,
        model: String,
    ) -> Result<ChatThreadBinding> {
        let (effective_provider_id, effective_model) = self
            .session_manager
            .update_thread_model(session_id, &thread_id, provider_id, &model)
            .await?;
        let (template_id, activated_provider_id, _activated_model) = self
            .session_manager
            .activate_thread(session_id, &thread_id)
            .await?;

        Ok(ChatThreadBinding {
            session_id,
            thread_id,
            template_id,
            effective_provider_id: Some(activated_provider_id.unwrap_or(effective_provider_id)),
            effective_model: Some(effective_model),
        })
    }

    pub async fn get_chat_messages(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Result<Vec<ChatMessage>> {
        self.session_manager
            .get_thread_messages(session_id, &thread_id)
            .await
    }

    pub async fn get_chat_thread_snapshot(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Result<ChatThreadSnapshot> {
        let (messages, turn_count, token_count, plan_item_count) = self
            .session_manager
            .get_thread_snapshot(session_id, &thread_id)
            .await?;
        Ok(ChatThreadSnapshot {
            session_id,
            thread_id,
            messages,
            turn_count,
            token_count,
            plan_item_count,
        })
    }

    pub async fn activate_chat_thread(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Result<ChatThreadBinding> {
        let (template_id, effective_provider_id, effective_model) = self
            .session_manager
            .activate_thread(session_id, &thread_id)
            .await?;
        Ok(ChatThreadBinding {
            session_id,
            thread_id,
            template_id,
            effective_provider_id,
            effective_model,
        })
    }

    pub async fn send_chat_message(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
        message: String,
    ) -> Result<()> {
        self.session_manager
            .send_message(session_id, &thread_id, message)
            .await
    }

    pub async fn cancel_chat_thread(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Result<()> {
        self.session_manager
            .cancel_thread(session_id, &thread_id)
            .await
    }

    pub async fn subscribe_chat_thread(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Result<broadcast::Receiver<ThreadEvent>> {
        self.session_manager
            .subscribe(session_id, &thread_id)
            .await
            .ok_or_else(|| ArgusError::ThreadNotFound(thread_id.to_string()))
    }

    pub async fn create_agent_run(
        &self,
        agent_id: AgentId,
        prompt: String,
        mcp_headers: McpRuntimeHeaderOverrides,
    ) -> Result<AgentRunSummary> {
        let has_run_context = !mcp_headers.is_empty();
        let session_id = self
            .session_manager
            .create(default_agent_run_session_name(agent_id))
            .await?;
        let thread_id = match self
            .session_manager
            .create_thread(session_id, agent_id, None, None)
            .await
        {
            Ok(thread_id) => thread_id,
            Err(error) => {
                self.cleanup_failed_chat_session(session_id).await;
                return Err(error);
            }
        };
        let run_id = AgentRunId::new();
        let mut events = match self.session_manager.subscribe(session_id, &thread_id).await {
            Some(events) => events,
            None => {
                self.cleanup_failed_agent_run_session(session_id, thread_id)
                    .await;
                return Err(ArgusError::ThreadNotFound(thread_id.to_string()));
            }
        };
        if has_run_context {
            self.agent_run_contexts
                .register_run_thread(run_id, thread_id, mcp_headers);
        }
        let thread_record = match self.session_manager.get_thread_record(&thread_id).await {
            Ok(thread_record) => thread_record,
            Err(error) => {
                if has_run_context {
                    self.agent_run_contexts.remove_run(run_id);
                }
                self.cleanup_failed_agent_run_session(session_id, thread_id)
                    .await;
                return Err(error);
            }
        };

        let record = AgentRunRecord {
            id: run_id,
            agent_id,
            session_id,
            thread_id,
            prompt: prompt.clone(),
            status: AgentRunStatus::Queued,
            result: None,
            error: None,
            created_at: thread_record.created_at.clone(),
            updated_at: thread_record.updated_at.clone(),
            completed_at: None,
        };
        if let Err(error) = self
            .agent_run_repo
            .insert_agent_run(&record)
            .await
            .map_err(database_error)
        {
            if has_run_context {
                self.agent_run_contexts.remove_run(run_id);
            }
            self.cleanup_failed_agent_run_session(session_id, thread_id)
                .await;
            return Err(error);
        }

        if let Err(error) = self
            .session_manager
            .send_message(session_id, &thread_id, prompt)
            .await
        {
            if has_run_context {
                self.agent_run_contexts.remove_run(run_id);
            }
            let _ = self.agent_run_repo.delete_agent_run(&run_id).await;
            self.cleanup_failed_agent_run_session(session_id, thread_id)
                .await;
            return Err(error);
        }

        let agent_run_repo = Arc::clone(&self.agent_run_repo);
        let session_manager = Arc::clone(&self.session_manager);
        let agent_run_contexts = self.agent_run_contexts.clone();
        tokio::spawn(async move {
            track_agent_run(
                &mut events,
                agent_run_repo,
                session_manager,
                session_id,
                thread_id,
                run_id,
                agent_run_contexts,
            )
            .await;
        });

        Ok(AgentRunSummary::from(&record))
    }

    pub async fn resolve_mcp_header_overrides(
        &self,
        raw_headers: BTreeMap<String, BTreeMap<String, String>>,
    ) -> std::result::Result<McpRuntimeHeaderOverrides, McpHeaderOverrideError> {
        if raw_headers.is_empty() {
            return Ok(McpRuntimeHeaderOverrides::empty());
        }

        let servers = self
            .mcp_repo
            .list_mcp_servers()
            .await
            .map_err(|error| McpHeaderOverrideError::Internal(database_error(error)))?;
        let mut overrides = McpRuntimeHeaderOverrides::empty();
        for (server_key, headers) in raw_headers {
            let server = resolve_mcp_server_for_runtime_headers(&servers, &server_key)
                .map_err(McpHeaderOverrideError::BadRequest)?;
            ensure_runtime_headers_supported(server).map_err(McpHeaderOverrideError::BadRequest)?;
            let mut runtime_headers = McpRuntimeHeaders::empty();
            for (name, value) in headers {
                validate_runtime_header(&name, &value)
                    .map_err(McpHeaderOverrideError::BadRequest)?;
                runtime_headers.insert(name, value);
            }
            if !runtime_headers.is_empty() {
                let server_id = server.id.ok_or_else(|| {
                    McpHeaderOverrideError::Internal(database_error(format!(
                        "MCP server '{}' is missing a persisted id",
                        server.display_name
                    )))
                })?;
                if overrides.get(server_id).is_some() {
                    return Err(McpHeaderOverrideError::BadRequest(format!(
                        "runtime headers for MCP server '{server_id}' were provided more than once"
                    )));
                }
                overrides.insert(server_id, runtime_headers);
            }
        }
        Ok(overrides)
    }

    pub async fn get_agent_run(&self, run_id: AgentRunId) -> Result<AgentRunDetail> {
        let record = self
            .agent_run_repo
            .get_agent_run(&run_id)
            .await
            .map_err(database_error)?
            .ok_or_else(|| ArgusError::ThreadNotFound(run_id.to_string()))?;

        self.refresh_agent_run_detail(record).await
    }

    async fn refresh_agent_run_detail(&self, mut record: AgentRunRecord) -> Result<AgentRunDetail> {
        if matches!(
            record.status,
            AgentRunStatus::Completed | AgentRunStatus::Failed
        ) {
            return Ok(AgentRunDetail::from(&record));
        }

        let messages = self
            .session_manager
            .get_thread_messages(record.session_id, &record.thread_id)
            .await
            .unwrap_or_default();
        let result = record
            .result
            .clone()
            .or_else(|| latest_assistant_message(&messages));
        let runtime_status = self
            .thread_pool_state()
            .runtimes
            .into_iter()
            .find(|runtime| runtime.thread_id == record.thread_id)
            .map(|runtime| runtime.status);
        let status = preserve_running_without_runtime(
            record.status,
            derive_agent_run_status(runtime_status, result.as_ref()),
        );

        if status != record.status || result != record.result {
            let now = timestamp_now();
            let completed_at = if matches!(status, AgentRunStatus::Completed) {
                record.completed_at.clone().or_else(|| Some(now.clone()))
            } else {
                record.completed_at.clone()
            };
            self.agent_run_repo
                .update_agent_run_status(
                    &record.id,
                    status,
                    result.as_deref(),
                    record.error.as_deref(),
                    completed_at.as_deref(),
                    &now,
                )
                .await
                .map_err(database_error)?;
            record.status = status;
            record.result = result;
            record.updated_at = now;
            record.completed_at = completed_at;
        }

        Ok(AgentRunDetail::from(&record))
    }
}

fn database_error(error: impl std::fmt::Display) -> ArgusError {
    ArgusError::DatabaseError {
        reason: error.to_string(),
    }
}

fn missing_after_mutation(kind: &str, id: impl std::fmt::Display) -> ArgusError {
    ArgusError::DatabaseError {
        reason: format!("{kind} not found after mutation: {id}"),
    }
}

fn default_agent_run_session_name(agent_id: AgentId) -> String {
    format!("Agent Run {}", agent_id.inner())
}

fn resolve_mcp_server_for_runtime_headers<'a>(
    servers: &'a [McpServerRecord],
    key: &str,
) -> std::result::Result<&'a McpServerRecord, String> {
    if let Ok(server_id) = key.parse::<i64>() {
        return servers
            .iter()
            .find(|server| server.id == Some(server_id))
            .ok_or_else(|| format!("MCP server '{key}' was not found"));
    }

    let matches = servers
        .iter()
        .filter(|server| server.display_name == key)
        .collect::<Vec<_>>();
    match matches.as_slice() {
        [server] => Ok(server),
        [] => Err(format!("MCP server '{key}' was not found")),
        _ => Err(format!("MCP server display name '{key}' is ambiguous")),
    }
}

fn ensure_runtime_headers_supported(server: &McpServerRecord) -> std::result::Result<(), String> {
    match server.transport {
        McpTransportConfig::Http { .. } | McpTransportConfig::Sse { .. } => Ok(()),
        McpTransportConfig::Stdio { .. } => Err(format!(
            "runtime headers only support HTTP/SSE MCP servers; '{}' uses stdio",
            server.display_name
        )),
    }
}

fn validate_runtime_header(name: &str, value: &str) -> std::result::Result<(), String> {
    HeaderName::from_bytes(name.as_bytes())
        .map_err(|error| format!("invalid MCP runtime header name '{name}': {error}"))?;
    HeaderValue::from_str(value)
        .map_err(|error| format!("invalid MCP runtime header value for '{name}': {error}"))?;
    Ok(())
}

fn timestamp_now() -> String {
    Utc::now().to_rfc3339()
}

fn latest_assistant_message(messages: &[ChatMessage]) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|message| {
            matches!(message.role, argus_protocol::llm::Role::Assistant)
                && !message.content.trim().is_empty()
        })
        .map(|message| message.content.clone())
}

fn derive_agent_run_status(
    runtime_status: Option<ThreadRuntimeStatus>,
    result: Option<&String>,
) -> AgentRunStatus {
    match runtime_status {
        Some(ThreadRuntimeStatus::Loading | ThreadRuntimeStatus::Queued) => AgentRunStatus::Queued,
        Some(ThreadRuntimeStatus::Running) => AgentRunStatus::Running,
        Some(ThreadRuntimeStatus::Cooling) => {
            if result.is_some() {
                AgentRunStatus::Completed
            } else {
                AgentRunStatus::Running
            }
        }
        Some(ThreadRuntimeStatus::Inactive | ThreadRuntimeStatus::Evicted) | None => {
            if result.is_some() {
                AgentRunStatus::Completed
            } else {
                AgentRunStatus::Queued
            }
        }
    }
}

fn preserve_running_without_runtime(
    current: AgentRunStatus,
    derived: AgentRunStatus,
) -> AgentRunStatus {
    if matches!(current, AgentRunStatus::Running) && matches!(derived, AgentRunStatus::Queued) {
        AgentRunStatus::Running
    } else {
        derived
    }
}

async fn mark_agent_run_running(agent_run_repo: &Arc<dyn AgentRunRepository>, run_id: AgentRunId) {
    let now = timestamp_now();
    if let Err(error) = agent_run_repo
        .update_agent_run_status(&run_id, AgentRunStatus::Running, None, None, None, &now)
        .await
    {
        tracing::warn!(%run_id, %error, "failed to mark agent run running");
    }
}

async fn mark_agent_run_failed(
    agent_run_repo: &Arc<dyn AgentRunRepository>,
    run_id: AgentRunId,
    error: String,
) {
    let now = timestamp_now();
    if let Err(db_error) = agent_run_repo
        .update_agent_run_status(
            &run_id,
            AgentRunStatus::Failed,
            None,
            Some(&error),
            Some(&now),
            &now,
        )
        .await
    {
        tracing::warn!(%run_id, error = %db_error, "failed to mark agent run failed");
    }
}

async fn mark_agent_run_completed(
    agent_run_repo: &Arc<dyn AgentRunRepository>,
    session_manager: &Arc<SessionManager>,
    session_id: SessionId,
    thread_id: ThreadId,
    run_id: AgentRunId,
) {
    let messages = match session_manager
        .get_thread_messages(session_id, &thread_id)
        .await
    {
        Ok(messages) => messages,
        Err(error) => {
            mark_agent_run_failed(agent_run_repo, run_id, error.to_string()).await;
            return;
        }
    };
    let result = latest_assistant_message(&messages);
    let now = timestamp_now();
    if let Err(error) = agent_run_repo
        .update_agent_run_status(
            &run_id,
            AgentRunStatus::Completed,
            result.as_deref(),
            None,
            Some(&now),
            &now,
        )
        .await
    {
        tracing::warn!(%run_id, %error, "failed to mark agent run completed");
    }
}

async fn track_agent_run(
    events: &mut broadcast::Receiver<ThreadEvent>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    session_manager: Arc<SessionManager>,
    session_id: SessionId,
    tracked_thread_id: ThreadId,
    run_id: AgentRunId,
    agent_run_contexts: AgentRunContextRegistry,
) {
    let thread_id_text = tracked_thread_id.to_string();
    loop {
        match events.recv().await {
            Ok(ThreadEvent::Processing { thread_id, .. })
            | Ok(ThreadEvent::ToolStarted { thread_id, .. })
            | Ok(ThreadEvent::ToolCompleted { thread_id, .. }) => {
                if thread_id == thread_id_text {
                    mark_agent_run_running(&agent_run_repo, run_id).await;
                }
            }
            Ok(ThreadEvent::TurnFailed {
                thread_id, error, ..
            }) => {
                if thread_id == thread_id_text {
                    mark_agent_run_failed(&agent_run_repo, run_id, error).await;
                }
            }
            Ok(ThreadEvent::TurnSettled { thread_id, .. }) => {
                if thread_id == thread_id_text {
                    mark_agent_run_completed(
                        &agent_run_repo,
                        &session_manager,
                        session_id,
                        tracked_thread_id,
                        run_id,
                    )
                    .await;
                }
            }
            Ok(ThreadEvent::Idle { thread_id }) => {
                if thread_id == thread_id_text {
                    break;
                }
            }
            Ok(_) => {}
            Err(broadcast::error::RecvError::Lagged(_)) => continue,
            Err(broadcast::error::RecvError::Closed) => break,
        }
    }
    agent_run_contexts.release_thread(tracked_thread_id);
}

#[cfg(test)]
mod tests {
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    struct RecordingMcpResolver {
        contexts: Mutex<Vec<McpToolResolutionContext>>,
    }

    #[async_trait]
    impl McpToolResolver for RecordingMcpResolver {
        async fn resolve_for_agent(
            &self,
            _agent_id: AgentId,
            context: &McpToolResolutionContext,
        ) -> Result<ResolvedMcpTools> {
            self.contexts.lock().unwrap().push(context.clone());
            Ok(ResolvedMcpTools::default())
        }
    }

    #[tokio::test]
    async fn run_scoped_resolver_uses_thread_headers() {
        let inner = Arc::new(RecordingMcpResolver::default());
        let registry = AgentRunContextRegistry::default();
        let run_id = AgentRunId::new();
        let thread_id = ThreadId::new();
        let mut headers = McpRuntimeHeaders::empty();
        headers.insert("Authorization", "Bearer runtime");
        let mut overrides = McpRuntimeHeaderOverrides::empty();
        overrides.insert(12, headers);
        registry.register_run_thread(run_id, thread_id, overrides.clone());

        let resolver = RunScopedMcpToolResolver::new(inner.clone(), registry);
        resolver
            .resolve_for_agent(
                AgentId::new(7),
                &McpToolResolutionContext::for_thread(thread_id),
            )
            .await
            .expect("resolver should delegate with run headers");

        let contexts = inner.contexts.lock().unwrap();
        assert_eq!(contexts.len(), 1);
        assert_eq!(contexts[0].thread_id, Some(thread_id));
        assert_eq!(contexts[0].runtime_headers, overrides);
    }

    #[tokio::test]
    async fn run_scoped_resolver_isolates_headers_between_concurrent_runs() {
        let inner = Arc::new(RecordingMcpResolver::default());
        let registry = AgentRunContextRegistry::default();
        let first_run = AgentRunId::new();
        let first_thread = ThreadId::new();
        let second_run = AgentRunId::new();
        let second_thread = ThreadId::new();

        let mut first_headers = McpRuntimeHeaders::empty();
        first_headers.insert("Authorization", "Bearer first");
        let mut first_overrides = McpRuntimeHeaderOverrides::empty();
        first_overrides.insert(12, first_headers);

        let mut second_headers = McpRuntimeHeaders::empty();
        second_headers.insert("Authorization", "Bearer second");
        let mut second_overrides = McpRuntimeHeaderOverrides::empty();
        second_overrides.insert(12, second_headers);

        registry.register_run_thread(first_run, first_thread, first_overrides.clone());
        registry.register_run_thread(second_run, second_thread, second_overrides.clone());

        let resolver = RunScopedMcpToolResolver::new(inner.clone(), registry);
        resolver
            .resolve_for_agent(
                AgentId::new(7),
                &McpToolResolutionContext::for_thread(first_thread),
            )
            .await
            .expect("first run should resolve with its own runtime headers");
        resolver
            .resolve_for_agent(
                AgentId::new(7),
                &McpToolResolutionContext::for_thread(second_thread),
            )
            .await
            .expect("second run should resolve with its own runtime headers");

        let contexts = inner.contexts.lock().unwrap();
        assert_eq!(contexts.len(), 2);
        assert_eq!(contexts[0].thread_id, Some(first_thread));
        assert_eq!(contexts[0].runtime_headers, first_overrides);
        assert_eq!(contexts[1].thread_id, Some(second_thread));
        assert_eq!(contexts[1].runtime_headers, second_overrides);
    }

    #[tokio::test]
    async fn track_agent_run_idle_keeps_child_thread_headers_alive() {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("sqlite pool should connect for tests");
        migrate(&pool)
            .await
            .expect("test migrations should succeed");
        let core = ServerCore::with_pool(pool)
            .await
            .expect("server core should initialize for tests");
        let run_id = AgentRunId::new();
        let parent_thread_id = ThreadId::new();
        let child_thread_id = ThreadId::new();
        let mut headers = McpRuntimeHeaders::empty();
        headers.insert("Authorization", "Bearer runtime");
        let mut overrides = McpRuntimeHeaderOverrides::empty();
        overrides.insert(12, headers);
        core.agent_run_contexts
            .register_run_thread(run_id, parent_thread_id, overrides.clone());
        core.agent_run_contexts
            .inherit_thread(parent_thread_id, child_thread_id)
            .expect("child thread should inherit run context");

        let (events_tx, mut events_rx) = broadcast::channel(8);
        let agent_run_repo = Arc::clone(&core.agent_run_repo);
        let session_manager = Arc::clone(&core.session_manager);
        let agent_run_contexts = core.agent_run_contexts.clone();
        let task = tokio::spawn(async move {
            track_agent_run(
                &mut events_rx,
                agent_run_repo,
                session_manager,
                SessionId::new(),
                parent_thread_id,
                run_id,
                agent_run_contexts,
            )
            .await;
        });

        events_tx
            .send(ThreadEvent::Idle {
                thread_id: parent_thread_id.to_string(),
            })
            .expect("idle event should send");
        task.await.expect("tracking task should complete");

        assert_eq!(
            core.agent_run_contexts.headers_for_thread(child_thread_id),
            overrides,
            "parent idle should not drop inherited child-thread headers"
        );
    }
}
