use std::sync::Arc;

use argus_auth::AccountManager;
use argus_crypto::{Cipher, FileKeySource};
use argus_job::JobManager;
use argus_llm::ProviderManager;
use argus_mcp::{McpRuntime, McpRuntimeConfig, RmcpConnector};
use argus_protocol::{
    AgentId, AgentRecord, ArgusError, JobRuntimeState, LlmProviderId, LlmProviderRecord,
    McpDiscoveredToolRecord, McpServerRecord, McpServerStatus, McpToolResolver, ProviderResolver,
    ProviderTestResult, Result, ThreadPoolState,
};
use argus_repository::traits::{
    AccountRepository, AdminSettingsRepository, AgentRepository, JobRepository,
    LlmProviderRepository, McpRepository, SessionRepository, ThreadRepository,
};
use argus_repository::types::AdminSettingsRecord;
use argus_repository::{ArgusSqlite, connect, connect_path, migrate};
use argus_session::SessionManager;
use argus_template::TemplateManager;
use argus_thread_pool::ThreadPool;
use argus_tool::ToolManager;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::sync::RwLock;

use crate::db::{DatabaseTarget, default_trace_dir, ensure_parent_dir, resolve_database_target};
use crate::resolver::ProviderManagerResolver;

const DEFAULT_AGENT_DISPLAY_NAME: &str = "ArgusWing";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdminSettings {
    pub instance_name: String,
}

impl Default for AdminSettings {
    fn default() -> Self {
        Self {
            instance_name: "ArgusWing".to_string(),
        }
    }
}

pub struct ServerCore {
    provider_manager: Arc<ProviderManager>,
    template_manager: Arc<TemplateManager>,
    session_manager: Arc<SessionManager>,
    _tool_manager: Arc<ToolManager>,
    job_manager: Arc<JobManager>,
    mcp_runtime: Arc<McpRuntime>,
    _account_manager: Arc<AccountManager>,
    mcp_repo: Arc<dyn McpRepository>,
    admin_settings_repo: Arc<dyn AdminSettingsRepository>,
    admin_settings: Arc<RwLock<AdminSettings>>,
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
        let admin_settings_repo: Arc<dyn AdminSettingsRepository> = sqlite.clone();
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

        let admin_settings = Arc::new(RwLock::new(AdminSettings::from(
            admin_settings_repo.get_admin_settings().await?,
        )));

        Ok(Arc::new(Self {
            provider_manager,
            template_manager,
            session_manager,
            _tool_manager: tool_manager,
            job_manager,
            mcp_runtime,
            _account_manager: account_manager,
            mcp_repo,
            admin_settings_repo,
            admin_settings,
        }))
    }

    async fn bootstrap_template_manager(template_manager: Arc<TemplateManager>) -> Result<()> {
        template_manager.repair_placeholder_ids().await?;
        template_manager.seed_builtin_agents().await?;
        Ok(())
    }

    pub async fn admin_settings(&self) -> AdminSettings {
        self.admin_settings.read().await.clone()
    }

    pub async fn update_admin_settings(&self, settings: AdminSettings) -> Result<()> {
        let saved = self
            .admin_settings_repo
            .upsert_admin_settings(&AdminSettingsRecord::from(settings))
            .await
            .map_err(database_error)?;
        *self.admin_settings.write().await = AdminSettings::from(saved);
        Ok(())
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

    pub fn thread_pool_state(&self) -> ThreadPoolState {
        self.session_manager.thread_pool_state()
    }

    pub fn job_runtime_state(&self) -> JobRuntimeState {
        self.job_manager.job_runtime_state()
    }
}

fn database_error(error: impl std::fmt::Display) -> ArgusError {
    ArgusError::DatabaseError {
        reason: error.to_string(),
    }
}

impl From<AdminSettingsRecord> for AdminSettings {
    fn from(value: AdminSettingsRecord) -> Self {
        Self {
            instance_name: value.instance_name,
        }
    }
}

impl From<AdminSettings> for AdminSettingsRecord {
    fn from(value: AdminSettings) -> Self {
        Self {
            instance_name: value.instance_name,
        }
    }
}
