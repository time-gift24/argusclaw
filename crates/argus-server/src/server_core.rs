use std::path::Path;
use std::sync::Arc;

use argus_auth::{AccountManager, AuthError};
use argus_crypto::{Cipher, FileKeySource, KeyMaterialSource};
use argus_job::JobManager;
use argus_llm::ProviderManager;
use argus_mcp::{McpRuntime, McpRuntimeConfig, RmcpConnector};
use argus_protocol::llm::ChatMessage;
use argus_protocol::{
    AgentId, AgentRecord, ArgusError, JobRuntimeState, LlmProviderId, LlmProviderRecord,
    McpDiscoveredToolRecord, McpServerRecord, McpServerStatus, McpToolResolver, ProviderId,
    ProviderResolver, ProviderTestResult, Result, RiskLevel, SessionId, ThreadEvent, ThreadId,
    ThreadPoolState, ThreadRuntimeStatus, UserId,
};
use argus_repository::traits::{
    AccountRepository, AgentRepository, AgentRunRepository, JobRepository, LlmProviderRepository,
    McpRepository, ResolvedUser, SessionRepository, TemplateRepairRepository, ThreadRepository,
    UserRepository,
};
use argus_repository::types::{AgentDeleteReport, AgentRunId, AgentRunRecord, AgentRunStatus};
use argus_repository::{ArgusPostgres, ArgusSqlite, connect_postgres, migrate_postgres};
use argus_session::scheduled_messages::{
    CreateScheduledMessageRequest, ScheduledMessageSummary, UpdateScheduledMessageRequest,
};
use argus_session::{SessionManager, SessionSummary, ThreadSummary};
use argus_template::{TemplateDeleteOptions, TemplateManager};
use argus_thread_pool::ThreadPool;
use argus_tool::ToolManager;
use chrono::Utc;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, SqlitePool};
use tokio::sync::broadcast;

use crate::db::{DatabaseTarget, resolve_database_target};
use crate::resolver::ProviderManagerResolver;
use crate::server_config::{ServerConfig, default_trace_dir};
use crate::user_context::RequestUser;

const DEFAULT_AGENT_DISPLAY_NAME: &str = "ArgusWing";
const DEFAULT_INSTANCE_NAME: &str = "ArgusWing";

pub struct ServerCore {
    provider_manager: Arc<ProviderManager>,
    template_manager: Arc<TemplateManager>,
    session_manager: Arc<SessionManager>,
    tool_manager: Arc<ToolManager>,
    job_manager: Arc<JobManager>,
    mcp_runtime: Arc<McpRuntime>,
    account_manager: Arc<AccountManager>,
    mcp_repo: Arc<dyn McpRepository>,
    agent_run_repo: Arc<dyn AgentRunRepository>,
    user_repo: Arc<dyn UserRepository>,
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
    pub async fn init(database_url: Option<&str>) -> Result<Arc<Self>> {
        let database_target = resolve_database_target(database_url)?;
        let DatabaseTarget::PostgresUrl(database_url) = database_target;
        let pool = connect_postgres(&database_url).await?;
        migrate_postgres(&pool).await?;
        Self::from_postgres_pool(
            pool,
            Path::new("~/.arguswing/master.key"),
            default_trace_dir(),
        )
        .await
    }

    pub async fn init_with_config(config: &ServerConfig) -> Result<Arc<Self>> {
        let database_target = resolve_database_target(Some(&config.database_url))?;
        let DatabaseTarget::PostgresUrl(database_url) = database_target;
        let pool = connect_postgres(&database_url).await?;
        migrate_postgres(&pool).await?;
        Self::from_postgres_pool(pool, &config.master_key_path, config.trace_dir.clone()).await
    }

    pub async fn with_pool(pool: SqlitePool) -> Result<Arc<Self>> {
        Self::from_sqlite_pool(pool).await
    }

    async fn from_postgres_pool(
        pool: PgPool,
        master_key_path: &Path,
        trace_dir: std::path::PathBuf,
    ) -> Result<Arc<Self>> {
        let key_source: Arc<dyn KeyMaterialSource> =
            Arc::new(FileKeySource::new(master_key_path.display().to_string()));
        let cipher = Arc::new(Cipher::new_arc(Arc::clone(&key_source)));
        let postgres = Arc::new(ArgusPostgres::with_key_sources(
            pool,
            key_source,
            Vec::new(),
        ));
        Self::from_repositories(
            postgres.clone() as Arc<dyn AccountRepository>,
            postgres.clone() as Arc<dyn LlmProviderRepository>,
            postgres.clone() as Arc<dyn AgentRepository>,
            postgres.clone() as Arc<dyn TemplateRepairRepository>,
            postgres.clone() as Arc<dyn JobRepository>,
            postgres.clone() as Arc<dyn ThreadRepository>,
            postgres.clone() as Arc<dyn SessionRepository>,
            postgres.clone() as Arc<dyn McpRepository>,
            postgres.clone() as Arc<dyn AgentRunRepository>,
            postgres.clone() as Arc<dyn UserRepository>,
            cipher,
            trace_dir,
        )
        .await
    }

    async fn from_sqlite_pool(pool: SqlitePool) -> Result<Arc<Self>> {
        let cipher = Arc::new(Cipher::new(FileKeySource::from_env_or_default()));
        let sqlite = Arc::new(ArgusSqlite::new(pool));
        Self::from_repositories(
            sqlite.clone() as Arc<dyn AccountRepository>,
            sqlite.clone() as Arc<dyn LlmProviderRepository>,
            sqlite.clone() as Arc<dyn AgentRepository>,
            sqlite.clone() as Arc<dyn TemplateRepairRepository>,
            sqlite.clone() as Arc<dyn JobRepository>,
            sqlite.clone() as Arc<dyn ThreadRepository>,
            sqlite.clone() as Arc<dyn SessionRepository>,
            sqlite.clone() as Arc<dyn McpRepository>,
            sqlite.clone() as Arc<dyn AgentRunRepository>,
            sqlite.clone() as Arc<dyn UserRepository>,
            cipher,
            default_trace_dir(),
        )
        .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn from_repositories(
        account_repo: Arc<dyn AccountRepository>,
        llm_repository: Arc<dyn LlmProviderRepository>,
        agent_repository: Arc<dyn AgentRepository>,
        template_repair_repository: Arc<dyn TemplateRepairRepository>,
        job_repository: Arc<dyn JobRepository>,
        thread_repository: Arc<dyn ThreadRepository>,
        session_repository: Arc<dyn SessionRepository>,
        mcp_repo: Arc<dyn McpRepository>,
        agent_run_repo: Arc<dyn AgentRunRepository>,
        user_repo: Arc<dyn UserRepository>,
        cipher: Arc<Cipher>,
        trace_dir: std::path::PathBuf,
    ) -> Result<Arc<Self>> {
        let account_manager = Arc::new(AccountManager::new(account_repo.clone(), cipher.clone()));
        let provider_manager =
            Arc::new(ProviderManager::new(llm_repository.clone()).with_auth(account_repo, cipher));

        let template_manager = Arc::new(TemplateManager::new(
            agent_repository,
            template_repair_repository,
        ));
        Self::bootstrap_template_manager(Arc::clone(&template_manager)).await?;

        let tool_manager = Arc::new(ToolManager::new());
        Self::register_default_tools(&tool_manager);
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
            Some(Arc::clone(&job_repository)),
            Some(Arc::clone(&thread_repository)),
            Some(Arc::clone(&llm_repository)),
        ));

        let mcp_runtime = Arc::new(McpRuntime::new(
            Arc::clone(&mcp_repo),
            Arc::new(RmcpConnector),
            McpRuntimeConfig::default(),
        ));
        McpRuntime::start(&mcp_runtime);
        let mcp_tool_resolver: Arc<dyn McpToolResolver> =
            Arc::new(McpRuntime::handle(&mcp_runtime));
        job_manager.set_mcp_tool_resolver(Some(Arc::clone(&mcp_tool_resolver)));

        let session_manager = Arc::new(SessionManager::new(
            session_repository,
            Arc::clone(&thread_repository),
            Arc::clone(&llm_repository),
            Arc::clone(&template_manager),
            provider_resolver,
            mcp_tool_resolver,
            Arc::clone(&tool_manager),
            trace_dir,
            thread_pool,
            Arc::clone(&job_manager),
            Some(Arc::clone(&job_repository)),
        ));

        Ok(Arc::new(Self {
            provider_manager,
            template_manager,
            session_manager,
            tool_manager,
            job_manager,
            mcp_runtime,
            account_manager,
            mcp_repo,
            agent_run_repo,
            user_repo,
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

    pub async fn get_account_username(&self) -> Result<Option<String>> {
        self.account_manager
            .get_current_user()
            .await
            .map(|user| user.map(|user| user.username))
            .map_err(auth_error_to_argus_error)
    }

    pub async fn configure_account(&self, username: &str, password: &str) -> Result<()> {
        self.account_manager
            .configure_account(username, password)
            .await
            .map_err(auth_error_to_argus_error)
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

    pub async fn delete_template_with_options(
        &self,
        id: AgentId,
        options: TemplateDeleteOptions,
    ) -> Result<AgentDeleteReport> {
        self.template_manager.delete_with_options(id, options).await
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

    pub async fn list_agent_mcp_bindings(
        &self,
        agent_id: AgentId,
    ) -> Result<Vec<argus_protocol::AgentMcpBinding>> {
        self.mcp_repo
            .list_agent_mcp_bindings(agent_id)
            .await
            .map_err(database_error)
    }

    pub async fn set_agent_mcp_bindings(
        &self,
        agent_id: AgentId,
        bindings: Vec<argus_protocol::AgentMcpBinding>,
    ) -> Result<()> {
        self.mcp_repo
            .set_agent_mcp_bindings(agent_id, &bindings)
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

    async fn resolve_request_user(&self, request_user: &RequestUser) -> Result<ResolvedUser> {
        self.user_repo
            .resolve_user(request_user.external_id(), request_user.display_name())
            .await
            .map_err(database_error)
    }

    async fn resolve_chat_user(&self, request_user: &RequestUser) -> Result<UserId> {
        Ok(self.resolve_request_user(request_user).await?.id)
    }

    pub async fn chat_user_id(&self, request_user: &RequestUser) -> Result<UserId> {
        self.resolve_chat_user(request_user).await
    }

    pub async fn is_request_user_admin(&self, request_user: &RequestUser) -> Result<bool> {
        Ok(self.resolve_request_user(request_user).await?.is_admin)
    }

    pub async fn current_user(&self, request_user: &RequestUser) -> Result<ResolvedUser> {
        self.resolve_request_user(request_user).await
    }

    pub async fn set_dev_user_admin(
        &self,
        external_id: &str,
        display_name: Option<&str>,
        is_admin: bool,
    ) -> Result<()> {
        self.user_repo
            .set_user_admin(external_id, display_name, is_admin)
            .await
            .map_err(database_error)?;
        Ok(())
    }

    async fn cleanup_failed_chat_session_for_user(&self, user_id: UserId, session_id: SessionId) {
        if let Err(error) = self
            .session_manager
            .delete_for_user(user_id, session_id)
            .await
        {
            tracing::warn!(
                session_id = %session_id,
                error = %error,
                "failed to clean up user chat session after materialization error"
            );
        }
    }

    pub async fn list_chat_sessions(
        &self,
        request_user: &RequestUser,
    ) -> Result<Vec<SessionSummary>> {
        tracing::trace!(external_user_id = %request_user.external_id(), "listing chat sessions for request user");
        let user_id = self.resolve_chat_user(request_user).await?;
        self.session_manager.list_sessions_for_user(user_id).await
    }

    pub async fn create_chat_session(
        &self,
        request_user: &RequestUser,
        name: String,
    ) -> Result<SessionSummary> {
        let user_id = self.resolve_chat_user(request_user).await?;
        let session_id = self.session_manager.create_for_user(user_id, name).await?;
        self.session_manager
            .list_sessions_for_user(user_id)
            .await?
            .into_iter()
            .find(|session| session.id == session_id)
            .ok_or_else(|| missing_after_mutation("session", session_id))
    }

    pub async fn delete_chat_session(
        &self,
        request_user: &RequestUser,
        session_id: SessionId,
    ) -> Result<()> {
        tracing::trace!(external_user_id = %request_user.external_id(), %session_id, "deleting chat session for request user");
        let user_id = self.resolve_chat_user(request_user).await?;
        self.session_manager
            .delete_for_user(user_id, session_id)
            .await
    }

    pub async fn rename_chat_session(
        &self,
        request_user: &RequestUser,
        session_id: SessionId,
        name: String,
    ) -> Result<SessionSummary> {
        let user_id = self.resolve_chat_user(request_user).await?;
        self.session_manager
            .rename_session_for_user(user_id, session_id, name)
            .await?;
        self.session_manager
            .list_sessions_for_user(user_id)
            .await?
            .into_iter()
            .find(|session| session.id == session_id)
            .ok_or_else(|| missing_after_mutation("session", session_id))
    }

    pub async fn list_chat_threads(
        &self,
        request_user: &RequestUser,
        session_id: SessionId,
    ) -> Result<Vec<ThreadSummary>> {
        tracing::trace!(external_user_id = %request_user.external_id(), %session_id, "listing chat threads for request user");
        let user_id = self.resolve_chat_user(request_user).await?;
        self.session_manager
            .list_threads_for_user(user_id, session_id)
            .await
    }

    pub async fn create_chat_thread(
        &self,
        request_user: &RequestUser,
        session_id: SessionId,
        template_id: AgentId,
        provider_id: Option<ProviderId>,
        model: Option<String>,
    ) -> Result<ThreadSummary> {
        tracing::trace!(external_user_id = %request_user.external_id(), %session_id, "creating chat thread for request user");
        let user_id = self.resolve_chat_user(request_user).await?;
        let thread_id = self
            .session_manager
            .create_thread_for_user(
                user_id,
                session_id,
                template_id,
                provider_id,
                model.as_deref(),
            )
            .await?;
        self.session_manager
            .list_threads_for_user(user_id, session_id)
            .await?
            .into_iter()
            .find(|thread| thread.id == thread_id)
            .ok_or_else(|| missing_after_mutation("thread", thread_id))
    }

    pub async fn create_chat_session_with_thread(
        &self,
        request_user: &RequestUser,
        session_name: String,
        template_id: AgentId,
        provider_id: Option<ProviderId>,
        model: Option<String>,
    ) -> Result<ChatSessionPayload> {
        tracing::trace!(external_user_id = %request_user.external_id(), "creating chat session with thread for request user");
        let user_id = self.resolve_chat_user(request_user).await?;
        let session_id = self
            .session_manager
            .create_for_user(user_id, session_name)
            .await?;
        let thread_id = match self
            .session_manager
            .create_thread_for_user(
                user_id,
                session_id,
                template_id,
                provider_id,
                model.as_deref(),
            )
            .await
        {
            Ok(thread_id) => thread_id,
            Err(error) => {
                self.cleanup_failed_chat_session_for_user(user_id, session_id)
                    .await;
                return Err(error);
            }
        };
        let binding = match self
            .activate_chat_thread(request_user, session_id, thread_id)
            .await
        {
            Ok(binding) => binding,
            Err(error) => {
                self.cleanup_failed_chat_session_for_user(user_id, session_id)
                    .await;
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

    pub async fn delete_chat_thread(
        &self,
        request_user: &RequestUser,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Result<()> {
        tracing::trace!(external_user_id = %request_user.external_id(), %session_id, %thread_id, "deleting chat thread for request user");
        let user_id = self.resolve_chat_user(request_user).await?;
        self.session_manager
            .delete_thread_for_user(user_id, session_id, &thread_id)
            .await
    }

    pub async fn rename_chat_thread(
        &self,
        request_user: &RequestUser,
        session_id: SessionId,
        thread_id: ThreadId,
        title: String,
    ) -> Result<ThreadSummary> {
        tracing::trace!(external_user_id = %request_user.external_id(), %session_id, %thread_id, "renaming chat thread for request user");
        let user_id = self.resolve_chat_user(request_user).await?;
        self.session_manager
            .rename_thread_for_user(user_id, session_id, &thread_id, title)
            .await?;
        self.session_manager
            .list_threads_for_user(user_id, session_id)
            .await?
            .into_iter()
            .find(|thread| thread.id == thread_id)
            .ok_or_else(|| missing_after_mutation("thread", thread_id))
    }

    pub async fn update_chat_thread_model(
        &self,
        request_user: &RequestUser,
        session_id: SessionId,
        thread_id: ThreadId,
        provider_id: ProviderId,
        model: String,
    ) -> Result<ChatThreadBinding> {
        tracing::trace!(external_user_id = %request_user.external_id(), %session_id, %thread_id, "updating chat thread model for request user");
        let user_id = self.resolve_chat_user(request_user).await?;
        let (effective_provider_id, effective_model) = self
            .session_manager
            .update_thread_model_for_user(user_id, session_id, &thread_id, provider_id, &model)
            .await?;
        let (template_id, activated_provider_id, _activated_model) = self
            .session_manager
            .activate_thread_for_user(user_id, session_id, &thread_id)
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
        request_user: &RequestUser,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Result<Vec<ChatMessage>> {
        tracing::trace!(external_user_id = %request_user.external_id(), %session_id, %thread_id, "getting chat messages for request user");
        let user_id = self.resolve_chat_user(request_user).await?;
        self.session_manager
            .get_thread_messages_for_user(user_id, session_id, &thread_id)
            .await
    }

    pub async fn get_chat_thread_snapshot(
        &self,
        request_user: &RequestUser,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Result<ChatThreadSnapshot> {
        tracing::trace!(external_user_id = %request_user.external_id(), %session_id, %thread_id, "getting chat thread snapshot for request user");
        let user_id = self.resolve_chat_user(request_user).await?;
        let (messages, turn_count, token_count, plan_item_count) = self
            .session_manager
            .get_thread_snapshot_for_user(user_id, session_id, &thread_id)
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
        request_user: &RequestUser,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Result<ChatThreadBinding> {
        tracing::trace!(external_user_id = %request_user.external_id(), %session_id, %thread_id, "activating chat thread for request user");
        let user_id = self.resolve_chat_user(request_user).await?;
        let (template_id, effective_provider_id, effective_model) = self
            .session_manager
            .activate_thread_for_user(user_id, session_id, &thread_id)
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
        request_user: &RequestUser,
        session_id: SessionId,
        thread_id: ThreadId,
        message: String,
    ) -> Result<()> {
        tracing::trace!(external_user_id = %request_user.external_id(), %session_id, %thread_id, "sending chat message for request user");
        let user_id = self.resolve_chat_user(request_user).await?;
        self.session_manager
            .send_message_for_user(user_id, session_id, &thread_id, message)
            .await
    }

    pub async fn cancel_chat_thread(
        &self,
        request_user: &RequestUser,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Result<()> {
        tracing::trace!(external_user_id = %request_user.external_id(), %session_id, %thread_id, "cancelling chat thread for request user");
        let user_id = self.resolve_chat_user(request_user).await?;
        self.session_manager
            .cancel_thread_for_user(user_id, session_id, &thread_id)
            .await
    }

    pub async fn list_scheduled_messages(
        &self,
        request_user: &RequestUser,
    ) -> Result<Vec<ScheduledMessageSummary>> {
        let user_id = self.resolve_chat_user(request_user).await?;
        self.session_manager
            .list_scheduled_messages_for_user(user_id)
            .await
    }

    pub async fn create_scheduled_message(
        &self,
        request_user: &RequestUser,
        mut request: CreateScheduledMessageRequest,
    ) -> Result<ScheduledMessageSummary> {
        let user_id = self.resolve_chat_user(request_user).await?;
        request.owner_user_id = user_id;
        self.session_manager.create_scheduled_message(request).await
    }

    pub async fn pause_scheduled_message(
        &self,
        request_user: &RequestUser,
        job_id: &str,
    ) -> Result<ScheduledMessageSummary> {
        let user_id = self.resolve_chat_user(request_user).await?;
        self.session_manager
            .pause_scheduled_message_for_user(user_id, job_id)
            .await
    }

    pub async fn update_scheduled_message(
        &self,
        request_user: &RequestUser,
        job_id: &str,
        request: UpdateScheduledMessageRequest,
    ) -> Result<ScheduledMessageSummary> {
        let user_id = self.resolve_chat_user(request_user).await?;
        self.session_manager
            .update_scheduled_message_for_user(user_id, job_id, request)
            .await
    }

    pub async fn delete_scheduled_message(
        &self,
        request_user: &RequestUser,
        job_id: &str,
    ) -> Result<bool> {
        let user_id = self.resolve_chat_user(request_user).await?;
        self.session_manager
            .delete_scheduled_message_for_user(user_id, job_id)
            .await
    }

    pub async fn trigger_scheduled_message_now(
        &self,
        request_user: &RequestUser,
        job_id: &str,
    ) -> Result<ScheduledMessageSummary> {
        let user_id = self.resolve_chat_user(request_user).await?;
        self.session_manager
            .trigger_scheduled_message_now_for_user(user_id, job_id)
            .await
    }

    pub async fn subscribe_chat_thread(
        &self,
        request_user: &RequestUser,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Result<broadcast::Receiver<ThreadEvent>> {
        tracing::trace!(external_user_id = %request_user.external_id(), %session_id, %thread_id, "subscribing to chat thread for request user");
        let user_id = self.resolve_chat_user(request_user).await?;
        self.session_manager
            .subscribe_for_user(user_id, session_id, &thread_id)
            .await
            .ok_or_else(|| ArgusError::ThreadNotFound(thread_id.to_string()))
    }

    pub async fn create_agent_run(
        &self,
        agent_id: AgentId,
        prompt: String,
    ) -> Result<AgentRunSummary> {
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
                self.cleanup_failed_chat_session(session_id).await;
                return Err(ArgusError::ThreadNotFound(thread_id.to_string()));
            }
        };
        let thread_record = match self.session_manager.get_thread_record(&thread_id).await {
            Ok(thread_record) => thread_record,
            Err(error) => {
                self.cleanup_failed_chat_session(session_id).await;
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
        self.agent_run_repo
            .insert_agent_run(&record)
            .await
            .map_err(database_error)?;

        if let Err(error) = self
            .session_manager
            .send_message(session_id, &thread_id, prompt)
            .await
        {
            let _ = self.agent_run_repo.delete_agent_run(&run_id).await;
            self.cleanup_failed_chat_session(session_id).await;
            return Err(error);
        }

        let agent_run_repo = Arc::clone(&self.agent_run_repo);
        let session_manager = Arc::clone(&self.session_manager);
        tokio::spawn(async move {
            track_agent_run(
                &mut events,
                agent_run_repo,
                session_manager,
                session_id,
                thread_id,
                run_id,
            )
            .await;
        });

        Ok(AgentRunSummary::from(&record))
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

fn timestamp_now() -> String {
    Utc::now().to_rfc3339()
}

fn auth_error_to_argus_error(error: AuthError) -> ArgusError {
    match error {
        AuthError::DatabaseError { reason } => ArgusError::DatabaseError { reason },
        other => ArgusError::LlmError {
            reason: format!("account auth error: {other}"),
        },
    }
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
}
