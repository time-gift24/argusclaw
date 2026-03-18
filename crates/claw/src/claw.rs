use std::sync::Arc;
use std::{env, path::Path, path::PathBuf};

use async_trait::async_trait;
use sqlx::SqlitePool;
use tokio::sync::broadcast;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

#[cfg(feature = "dev")]
use crate::agents::Agent;
use crate::agents::builtins::{DEFAULT_AGENT_DISPLAY_NAME, load_arguswing};
use crate::agents::thread::ThreadInfo;
use crate::agents::{AgentId, AgentManager, AgentRecord, ThreadConfig};
use crate::db::llm::{LlmProviderId, LlmProviderRecord, LlmProviderSummary, ProviderTestResult};
use crate::db::sqlite::{
    SqliteAgentRepository, SqliteJobRepository, SqliteLlmProviderRepository, connect, connect_path,
    migrate,
};
use crate::error::AgentError;
use crate::job::JobRepository;
use crate::protocol::{ApprovalDecision, ThreadEvent, ThreadId};
use crate::scheduler::{Scheduler, SchedulerConfig};
use crate::user::UserService;
use argus_llm::ProviderManager;
#[cfg(feature = "dev")]
use argus_protocol::llm::LlmEventStream;
use argus_protocol::{ArgusError, LlmProvider, ProviderId};
use argus_tool::ToolManager;

// Session layer - use existing protocol types
use argus_log::SqliteTurnLogRepository;
use argus_protocol::SessionId;
use argus_session::ProviderResolver as ProviderResolverTrait;
use argus_session::{SessionManager, SessionSummary, ThreadSummary};
use argus_template::TemplateManager;
use argus_thread::CompactorManager;

/// Ensures the default ArgusWing agent exists in the database.
async fn ensure_default_agent(agent_manager: &AgentManager) -> Result<(), AgentError> {
    let default_agent = load_arguswing().map_err(|e| AgentError::BuiltinAgentLoadFailed {
        reason: e.to_string(),
    })?;
    // Look up by display_name to see if it already exists
    let existing = agent_manager
        .find_template_by_display_name(&default_agent.display_name)
        .await?;

    if existing.is_none() {
        // Insert the default agent (ID will be auto-generated)
        agent_manager.upsert_template(default_agent).await?;
    }
    Ok(())
}

/// Wrapper that implements ProviderResolver for ProviderManager.
///
/// This bridges the argus-session ProviderResolver trait with argus-llm's ProviderManager.
pub struct ProviderManagerResolver {
    provider_manager: Arc<ProviderManager>,
}

impl ProviderManagerResolver {
    /// Create a new resolver wrapper.
    pub fn new(provider_manager: Arc<ProviderManager>) -> Self {
        Self { provider_manager }
    }
}

#[async_trait]
impl ProviderResolverTrait for ProviderManagerResolver {
    async fn resolve(
        &self,
        id: ProviderId,
    ) -> std::result::Result<Arc<dyn LlmProvider>, ArgusError> {
        // Convert argus-protocol ProviderId to argus-protocol LlmProviderId
        let provider_id = LlmProviderId::new(id.inner());
        self.provider_manager.get_provider(&provider_id).await
    }
}

#[derive(Clone)]
pub struct AppContext {
    db_pool: SqlitePool,
    provider_manager: Arc<ProviderManager>,
    agent_manager: Arc<AgentManager>,
    tool_manager: Arc<ToolManager>,
    job_repository: Arc<dyn JobRepository>,
    shutdown: CancellationToken,
    user: UserService,
    // Session layer
    session_manager: Arc<SessionManager>,
    turn_log_repository: Arc<SqliteTurnLogRepository>,
}

impl AppContext {
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

        let user = UserService::new(pool.clone());

        let provider_manager = Arc::new(ProviderManager::new(llm_repository));
        let tool_manager = Arc::new(ToolManager::new());
        let agent_manager = Arc::new(AgentManager::new(
            agent_repository,
            provider_manager.clone(),
            tool_manager.clone(),
            None,
        ));

        // Ensure default agent exists
        ensure_default_agent(&agent_manager).await?;

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

        // Initialize session layer
        let template_manager = Arc::new(TemplateManager::new(pool.clone()));
        let provider_resolver = Arc::new(ProviderManagerResolver::new(provider_manager.clone()));
        let compactor_manager = Arc::new(CompactorManager::with_defaults());
        let session_manager = Arc::new(SessionManager::new(
            pool.clone(),
            template_manager,
            provider_resolver,
            tool_manager.clone(),
            compactor_manager,
        ));
        let turn_log_repository = Arc::new(SqliteTurnLogRepository::new(pool.clone()));

        Ok(Self {
            db_pool: pool,
            provider_manager,
            agent_manager,
            tool_manager,
            job_repository,
            shutdown,
            user,
            session_manager,
            turn_log_repository,
        })
    }

    #[must_use]
    pub fn new(
        provider_manager: Arc<ProviderManager>,
        agent_manager: Arc<AgentManager>,
        tool_manager: Arc<ToolManager>,
    ) -> Self {
        let pool = SqlitePool::connect_lazy_with(
            sqlx::sqlite::SqliteConnectOptions::new().filename(std::path::Path::new(":memory:")),
        );
        let provider_resolver = Arc::new(ProviderManagerResolver::new(provider_manager.clone()));
        let compactor_manager = Arc::new(CompactorManager::with_defaults());
        let session_manager = Arc::new(SessionManager::new(
            pool.clone(),
            Arc::new(TemplateManager::new(pool.clone())),
            provider_resolver,
            tool_manager.clone(),
            compactor_manager,
        ));
        Self {
            db_pool: pool.clone(),
            provider_manager,
            agent_manager,
            tool_manager,
            job_repository: Arc::new(SqliteJobRepository::new(pool.clone())),
            shutdown: CancellationToken::new(),
            user: UserService::new(pool.clone()),
            session_manager,
            turn_log_repository: Arc::new(SqliteTurnLogRepository::new(pool.clone())),
        }
    }

    /// Create a new AppContext with an explicit database pool.
    #[must_use]
    pub fn with_pool(
        pool: SqlitePool,
        provider_manager: Arc<ProviderManager>,
        agent_manager: Arc<AgentManager>,
        tool_manager: Arc<ToolManager>,
    ) -> Self {
        let provider_resolver = Arc::new(ProviderManagerResolver::new(provider_manager.clone()));
        let compactor_manager = Arc::new(CompactorManager::with_defaults());
        let session_manager = Arc::new(SessionManager::new(
            pool.clone(),
            Arc::new(TemplateManager::new(pool.clone())),
            provider_resolver,
            tool_manager.clone(),
            compactor_manager,
        ));
        Self {
            db_pool: pool.clone(),
            provider_manager,
            agent_manager,
            tool_manager,
            job_repository: Arc::new(SqliteJobRepository::new(pool.clone())),
            shutdown: CancellationToken::new(),
            user: UserService::new(pool.clone()),
            session_manager,
            turn_log_repository: Arc::new(SqliteTurnLogRepository::new(pool.clone())),
        }
    }

    #[cfg(feature = "dev")]
    #[must_use]
    pub fn db_pool(&self) -> &SqlitePool {
        &self.db_pool
    }

    #[cfg(feature = "dev")]
    #[must_use]
    pub fn provider_manager(&self) -> Arc<ProviderManager> {
        Arc::clone(&self.provider_manager)
    }

    #[cfg(feature = "dev")]
    #[must_use]
    pub fn agent_manager(&self) -> Arc<AgentManager> {
        Arc::clone(&self.agent_manager)
    }

    #[must_use]
    pub fn tool_manager(&self) -> Arc<ToolManager> {
        Arc::clone(&self.tool_manager)
    }

    /// Get the user service for authentication operations.
    pub fn user(&self) -> &UserService {
        &self.user
    }

    #[cfg(feature = "dev")]
    #[must_use]
    pub fn job_repository(&self) -> Arc<dyn JobRepository> {
        Arc::clone(&self.job_repository)
    }

    /// Trigger graceful shutdown of the scheduler.
    pub fn shutdown(&self) {
        self.shutdown.cancel();
    }

    // === Session Management API ===

    /// List all sessions.
    pub async fn list_sessions(&self) -> Result<Vec<SessionSummary>, AgentError> {
        self.session_manager
            .list_sessions()
            .await
            .map_err(|e| AgentError::Session {
                reason: e.to_string(),
            })
    }

    /// Create a new session.
    pub async fn create_session(&self, name: String) -> Result<SessionId, AgentError> {
        self.session_manager
            .create(name)
            .await
            .map_err(|e| AgentError::Session {
                reason: e.to_string(),
            })
    }

    /// Load a session into memory.
    pub async fn load_session(&self, session_id: SessionId) -> Result<(), AgentError> {
        self.session_manager
            .load(session_id)
            .await
            .map_err(|e| AgentError::Session {
                reason: e.to_string(),
            })?;
        Ok(())
    }

    /// Unload a session from memory.
    pub async fn unload_session(&self, session_id: SessionId) -> Result<(), AgentError> {
        self.session_manager
            .unload(session_id)
            .await
            .map_err(|e| AgentError::Session {
                reason: e.to_string(),
            })
    }

    /// Delete a session.
    pub async fn delete_session(&self, session_id: SessionId) -> Result<(), AgentError> {
        self.session_manager
            .delete(session_id)
            .await
            .map_err(|e| AgentError::Session {
                reason: e.to_string(),
            })
    }

    /// Create a new thread in a session.
    pub async fn create_thread_in_session(
        &self,
        session_id: SessionId,
        template_id: AgentId,
        provider_id: LlmProviderId,
    ) -> Result<ThreadId, AgentError> {
        // Convert to argus-protocol types for session manager
        let template_id_proto = argus_protocol::AgentId::new(template_id.into_inner());
        let provider_id_proto = argus_protocol::ProviderId::new(provider_id.into_inner());

        let thread_id = self
            .session_manager
            .create_thread(session_id, template_id_proto, provider_id_proto)
            .await
            .map_err(|e| AgentError::Session {
                reason: e.to_string(),
            })?;

        // Convert back to claw's ThreadId
        ThreadId::parse(&thread_id.inner().to_string()).map_err(|e| AgentError::Session {
            reason: e.to_string(),
        })
    }

    /// Delete a thread from a session.
    pub async fn delete_thread_from_session(
        &self,
        session_id: SessionId,
        thread_id: &ThreadId,
    ) -> Result<(), AgentError> {
        // Convert to argus-protocol ThreadId
        let thread_id_proto =
            argus_protocol::ThreadId::parse(&thread_id.to_string()).map_err(|e| {
                AgentError::Session {
                    reason: e.to_string(),
                }
            })?;

        self.session_manager
            .delete_thread(session_id, &thread_id_proto)
            .await
            .map_err(|e| AgentError::Session {
                reason: e.to_string(),
            })
    }

    /// List threads in a session.
    pub async fn list_threads_in_session(
        &self,
        session_id: SessionId,
    ) -> Result<Vec<ThreadSummary>, AgentError> {
        self.session_manager
            .list_threads(session_id)
            .await
            .map_err(|e| AgentError::Session {
                reason: e.to_string(),
            })
    }

    /// Run log cleanup (LRU-based).
    pub async fn cleanup_turn_logs(&self) -> Result<i64, AgentError> {
        use argus_log::LogCleaner;
        use std::sync::Arc;

        // Create a new cleaner with the right type
        let pool = self.db_pool.clone();
        let repo = Arc::new(SqliteTurnLogRepository::new(pool));
        let cleaner = LogCleaner::new(repo, 20);
        let report = cleaner.cleanup().await.map_err(|e| AgentError::Session {
            reason: e.to_string(),
        })?;
        Ok(report.deleted_count)
    }

    pub async fn upsert_provider(
        &self,
        record: LlmProviderRecord,
    ) -> Result<LlmProviderId, AgentError> {
        self.provider_manager
            .upsert_provider(record)
            .await
            .map_err(|e| AgentError::Provider {
                reason: e.to_string(),
            })
    }

    pub async fn delete_provider(&self, id: &LlmProviderId) -> Result<bool, AgentError> {
        self.provider_manager
            .delete_provider(id)
            .await
            .map_err(|e| AgentError::Provider {
                reason: e.to_string(),
            })
    }

    pub async fn import_providers(
        &self,
        records: Vec<LlmProviderRecord>,
    ) -> Result<(), AgentError> {
        self.provider_manager
            .import_providers(records)
            .await
            .map_err(|e| AgentError::Provider {
                reason: e.to_string(),
            })
    }

    pub async fn get_provider_record(
        &self,
        id: &LlmProviderId,
    ) -> Result<LlmProviderRecord, AgentError> {
        self.provider_manager
            .get_provider_record(id)
            .await
            .map_err(|e| AgentError::Provider {
                reason: e.to_string(),
            })
    }

    pub async fn get_provider_summary(
        &self,
        id: &LlmProviderId,
    ) -> Result<LlmProviderSummary, AgentError> {
        self.provider_manager
            .get_provider_summary(id)
            .await
            .map_err(|e| AgentError::Provider {
                reason: e.to_string(),
            })
    }

    pub async fn get_default_provider_record(&self) -> Result<LlmProviderRecord, AgentError> {
        self.provider_manager
            .get_default_provider_record()
            .await
            .map_err(|e| AgentError::Provider {
                reason: e.to_string(),
            })
    }

    pub async fn set_default_provider(&self, id: &LlmProviderId) -> Result<(), AgentError> {
        self.provider_manager
            .set_default_provider(id)
            .await
            .map_err(|e| AgentError::Provider {
                reason: e.to_string(),
            })
    }

    /// 获取 LLM Provider 实例
    pub async fn get_provider(
        &self,
        id: &LlmProviderId,
    ) -> Result<Arc<dyn LlmProvider>, AgentError> {
        self.provider_manager
            .get_provider(id)
            .await
            .map_err(|e| AgentError::Provider {
                reason: e.to_string(),
            })
    }

    /// 获取 LLM Provider 实例并指定模型
    pub async fn get_provider_with_model(
        &self,
        id: &LlmProviderId,
        model: &str,
    ) -> Result<Arc<dyn LlmProvider>, AgentError> {
        self.provider_manager
            .get_provider_with_model(id, model)
            .await
            .map_err(|e| AgentError::Provider {
                reason: e.to_string(),
            })
    }

    /// 获取默认 LLM Provider
    pub async fn get_default_provider(&self) -> Result<Arc<dyn LlmProvider>, AgentError> {
        self.provider_manager
            .get_default_provider()
            .await
            .map_err(|e| AgentError::Provider {
                reason: e.to_string(),
            })
    }

    /// 列出所有 provider 摘要
    pub async fn list_providers(&self) -> Result<Vec<LlmProviderSummary>, AgentError> {
        self.provider_manager
            .list_providers()
            .await
            .map_err(|e| AgentError::Provider {
                reason: e.to_string(),
            })
    }

    pub async fn test_provider_connection(
        &self,
        id: &LlmProviderId,
        model: &str,
    ) -> Result<ProviderTestResult, AgentError> {
        self.provider_manager
            .test_provider_connection(id, model)
            .await
            .map_err(|e| AgentError::Provider {
                reason: e.to_string(),
            })
    }

    pub async fn test_provider_record(
        &self,
        record: LlmProviderRecord,
        model: &str,
    ) -> Result<ProviderTestResult, AgentError> {
        self.provider_manager
            .test_provider_record(record, model)
            .await
            .map_err(|e| AgentError::Provider {
                reason: e.to_string(),
            })
    }

    pub async fn upsert_template(&self, record: AgentRecord) -> Result<(), AgentError> {
        self.agent_manager
            .upsert_template(record)
            .await
            .map_err(Into::into)
    }

    pub async fn get_template(&self, id: &AgentId) -> Result<Option<AgentRecord>, AgentError> {
        self.agent_manager
            .get_template(id)
            .await
            .map_err(Into::into)
    }

    pub async fn list_templates(&self) -> Result<Vec<AgentRecord>, AgentError> {
        self.agent_manager
            .list_templates()
            .await
            .map_err(Into::into)
    }

    pub async fn delete_template(&self, id: &AgentId) -> Result<bool, AgentError> {
        self.agent_manager
            .delete_template(id)
            .await
            .map_err(Into::into)
    }

    /// Get the default ArgusWing agent template.
    ///
    /// This agent is guaranteed to exist after `AppContext::init()`.
    pub async fn get_default_agent_template(&self) -> Result<AgentRecord, AgentError> {
        self.agent_manager
            .find_template_by_display_name(DEFAULT_AGENT_DISPLAY_NAME)
            .await?
            .ok_or(AgentError::DefaultAgentNotFound)
    }

    /// Create a runtime agent from the default template.
    ///
    /// Binds to the default LLM provider at runtime.
    ///
    /// # Errors
    ///
    /// Returns `DefaultProviderNotConfigured` if no default provider is set.
    pub async fn create_default_agent(&self) -> Result<AgentId, AgentError> {
        let template = self.get_default_agent_template().await?;
        let default_provider = self.get_default_provider_record().await?;
        let mut record = template;
        record.provider_id = Some(default_provider.id);
        self.agent_manager.create_agent(&record).await
    }

    /// Create a runtime agent from the default template with approval configuration.
    ///
    /// Binds to the default LLM provider at runtime.
    ///
    /// # Errors
    ///
    /// Returns `DefaultProviderNotConfigured` if no default provider is set.
    pub async fn create_default_agent_with_approval(
        &self,
        approval_tools: Vec<String>,
        auto_approve: bool,
    ) -> Result<AgentId, AgentError> {
        let template = self.get_default_agent_template().await?;
        let default_provider = self.get_default_provider_record().await?;
        let mut record = template;
        record.provider_id = Some(default_provider.id);
        self.agent_manager
            .create_agent_with_approval(&record, approval_tools, auto_approve)
            .await
    }

    /// Create a runtime agent from a template with an optional provider override.
    ///
    /// This method clones the template and binds to either the specified provider
    /// or the default provider.
    ///
    /// # Arguments
    ///
    /// * `template_id` - The ID of the template to clone
    /// * `provider_override` - Optional provider ID to use instead of the default
    ///
    /// # Errors
    ///
    /// Returns `AgentNotFound` if the template doesn't exist.
    /// Returns `DefaultProviderNotConfigured` if no override is provided and no default provider is set.
    pub async fn create_runtime_agent_from_template(
        &self,
        template_id: &AgentId,
        provider_override: Option<&LlmProviderId>,
    ) -> Result<crate::RuntimeAgentHandle, AgentError> {
        let record = self
            .get_template(template_id)
            .await?
            .ok_or(AgentError::AgentNotFound { id: *template_id })?;

        let effective_provider = match provider_override {
            Some(provider_id) => *provider_id,
            None => self.get_default_provider_record().await?.id,
        };

        let mut runtime_record = record;
        runtime_record.provider_id = Some(effective_provider);

        // Collect high/critical risk tools for approval
        let approval_tools: Vec<String> = runtime_record
            .tool_names
            .iter()
            .filter(|tool_name| match self.tool_manager.get(tool_name) {
                Some(tool) => {
                    matches!(
                        tool.risk_level(),
                        crate::RiskLevel::High | crate::RiskLevel::Critical
                    )
                }
                None => true,
            })
            .cloned()
            .collect();

        let runtime_agent_id = if approval_tools.is_empty() {
            self.agent_manager.create_agent(&runtime_record).await?
        } else {
            self.agent_manager
                .create_agent_with_approval(&runtime_record, approval_tools, false)
                .await?
        };
        Ok(crate::RuntimeAgentHandle {
            runtime_agent_id,
            template_id: *template_id,
            effective_provider_id: effective_provider,
        })
    }

    // === Agent Use-Case Methods ===

    /// Create a runtime Agent from an AgentRecord template.
    ///
    /// The agent must have a valid `provider_id` that references an existing LLM provider.
    pub async fn create_agent(&self, record: &AgentRecord) -> Result<AgentId, AgentError> {
        self.agent_manager.create_agent(record).await
    }

    /// Create a runtime Agent with approval configuration.
    ///
    /// This is a convenience method for creating agents that need approval tools.
    /// The agent must have a valid `provider_id` that references an existing LLM provider.
    pub async fn create_agent_with_approval(
        &self,
        record: &AgentRecord,
        approval_tools: Vec<String>,
        auto_approve: bool,
    ) -> Result<AgentId, AgentError> {
        self.agent_manager
            .create_agent_with_approval(record, approval_tools, auto_approve)
            .await
    }

    /// Get an existing Agent by ID (dev only).
    ///
    /// Returns `None` if the agent doesn't exist or hasn't been created yet.
    #[cfg(feature = "dev")]
    #[must_use]
    pub fn get_agent(&self, id: &AgentId) -> Option<Agent> {
        self.agent_manager.get(id)
    }

    /// List all active runtime agents.
    #[must_use]
    pub fn list_active_agents(&self) -> Vec<crate::agents::AgentRuntimeInfo> {
        self.agent_manager.list_agents()
    }

    /// Create a new thread in an agent.
    pub fn create_thread(
        &self,
        agent_id: &AgentId,
        config: ThreadConfig,
    ) -> Result<ThreadId, AgentError> {
        self.agent_manager.create_thread(agent_id, config)
    }

    /// List all threads in an agent.
    #[must_use]
    pub fn list_threads(&self, agent_id: &AgentId) -> Option<Vec<ThreadInfo>> {
        self.agent_manager.list_threads(agent_id)
    }

    /// Send a message to a thread.
    pub async fn send_message(
        &self,
        agent_id: &AgentId,
        thread_id: &ThreadId,
        message: String,
    ) -> Result<(), AgentError> {
        self.agent_manager
            .send_message(agent_id, thread_id, message)
            .await
    }

    /// Subscribe to thread events.
    pub async fn subscribe(
        &self,
        agent_id: &AgentId,
        thread_id: &ThreadId,
    ) -> Option<broadcast::Receiver<ThreadEvent>> {
        self.agent_manager.subscribe(agent_id, thread_id).await
    }

    /// Resolve an approval request.
    pub fn resolve_approval(
        &self,
        agent_id: &AgentId,
        request_id: Uuid,
        decision: ApprovalDecision,
        resolved_by: Option<String>,
    ) -> Result<(), AgentError> {
        self.agent_manager
            .resolve_approval(agent_id, request_id, decision, resolved_by)
    }

    /// Get a snapshot of a thread's current state.
    ///
    /// Returns the thread's messages, turn count, and token count.
    ///
    /// # Errors
    ///
    /// Returns `ThreadNotFound` if the thread doesn't exist.
    pub async fn get_thread_snapshot(
        &self,
        runtime_agent_id: &AgentId,
        thread_id: &ThreadId,
    ) -> Result<crate::ThreadSnapshot, AgentError> {
        self.agent_manager
            .get_thread_snapshot(runtime_agent_id, thread_id)
            .await
            .ok_or(AgentError::ThreadNotFound { id: *thread_id })
    }

    // === Dev-Only Methods ===

    #[cfg(feature = "dev")]
    pub async fn complete_text(
        &self,
        provider_id: Option<&LlmProviderId>,
        prompt: impl Into<String>,
    ) -> Result<String, AgentError> {
        self.provider_manager
            .complete_text(provider_id, prompt)
            .await
            .map_err(|e| AgentError::Provider {
                reason: e.to_string(),
            })
    }

    #[cfg(feature = "dev")]
    pub async fn stream_text(
        &self,
        provider_id: Option<&LlmProviderId>,
        prompt: impl Into<String>,
    ) -> Result<LlmEventStream, AgentError> {
        self.provider_manager
            .stream_text(provider_id, prompt)
            .await
            .map_err(|e| AgentError::Provider {
                reason: e.to_string(),
            })
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
    env::var("DATABASE_URL").unwrap_or_else(|_| "~/.arguswing/sqlite.db".to_string())
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
    use tempfile::tempdir;

    use super::{AppContext, expand_home_path, resolve_database_target};

    #[test]
    fn resolve_database_target_keeps_sqlite_urls() {
        let target = resolve_database_target(Some("sqlite::memory:".to_string()))
            .expect("sqlite urls should resolve");

        assert!(matches!(target, super::DatabaseTarget::Url(url) if url == "sqlite::memory:"));
    }

    #[test]
    fn expand_home_path_resolves_tilde_prefix() {
        let path = expand_home_path("~/.arguswing/sqlite.db").expect("home path should resolve");

        assert!(path.ends_with(".arguswing/sqlite.db"));
    }

    #[tokio::test]
    async fn init_creates_an_app_context_from_a_filesystem_database_path() {
        let temp_dir = tempdir().expect("temp dir should exist");
        let database_path = temp_dir.path().join("nested").join("sqlite.db");

        let ctx = AppContext::init(Some(database_path.display().to_string()))
            .await
            .expect("app context init should succeed");
        let providers = ctx
            .list_providers()
            .await
            .expect("provider list should succeed");

        assert!(providers.is_empty());
        assert!(database_path.exists());
    }

    #[tokio::test]
    async fn init_creates_default_arguswing_agent() {
        let temp_dir = tempdir().expect("temp dir should exist");
        let database_path = temp_dir.path().join("sqlite.db");

        let ctx = AppContext::init(Some(database_path.display().to_string()))
            .await
            .expect("app context init should succeed");

        let default_agent = ctx
            .get_default_agent_template()
            .await
            .expect("default agent should exist");

        assert_eq!(default_agent.display_name, "ArgusWing");
        assert!(default_agent.provider_id.is_none());
    }

    #[tokio::test]
    async fn create_runtime_agent_from_template_keeps_duplicate_templates_alive() {
        use std::collections::HashMap;

        let temp_dir = tempdir().expect("temp dir should exist");
        let database_path = temp_dir.path().join("sqlite.db");
        let ctx = AppContext::init(Some(database_path.display().to_string()))
            .await
            .expect("app context init should succeed");

        ctx.upsert_provider(crate::LlmProviderRecord {
            id: crate::LlmProviderId::new(1),
            kind: crate::LlmProviderKind::OpenAiCompatible,
            display_name: "OpenAI".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: crate::SecretString::new("sk-test"),
            models: vec!["gpt-4.1".to_string()],
            default_model: "gpt-4.1".to_string(),
            is_default: true,
            extra_headers: HashMap::new(),
            secret_status: crate::ProviderSecretStatus::Ready,
        })
        .await
        .expect("provider should save");

        let default_template = ctx
            .get_default_agent_template()
            .await
            .expect("default agent should exist");

        let first = ctx
            .create_runtime_agent_from_template(&default_template.id, None)
            .await
            .expect("first runtime agent should be created");
        let second = ctx
            .create_runtime_agent_from_template(&default_template.id, None)
            .await
            .expect("second runtime agent should be created");

        assert_ne!(first.runtime_agent_id, second.runtime_agent_id);
        assert_eq!(first.template_id, default_template.id);
        assert_eq!(second.template_id, default_template.id);
        assert_eq!(ctx.list_active_agents().len(), 2);
    }

    #[tokio::test]
    async fn get_thread_snapshot_returns_live_history_for_runtime_agent() {
        use std::collections::HashMap;

        let temp_dir = tempdir().expect("temp dir should exist");
        let database_path = temp_dir.path().join("sqlite.db");
        let ctx = AppContext::init(Some(database_path.display().to_string()))
            .await
            .expect("app context init should succeed");

        ctx.upsert_provider(crate::LlmProviderRecord {
            id: crate::LlmProviderId::new(1),
            kind: crate::LlmProviderKind::OpenAiCompatible,
            display_name: "OpenAI".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: crate::SecretString::new("sk-test"),
            models: vec!["gpt-4.1".to_string()],
            default_model: "gpt-4.1".to_string(),
            is_default: true,
            extra_headers: HashMap::new(),
            secret_status: crate::ProviderSecretStatus::Ready,
        })
        .await
        .expect("provider should save");

        let default_template = ctx
            .get_default_agent_template()
            .await
            .expect("default agent should exist");

        let runtime = ctx
            .create_runtime_agent_from_template(&default_template.id, None)
            .await
            .expect("runtime agent should be created");
        let thread_id = ctx
            .create_thread(&runtime.runtime_agent_id, crate::ThreadConfig::default())
            .expect("thread should be created");

        let snapshot = ctx
            .get_thread_snapshot(&runtime.runtime_agent_id, &thread_id)
            .await
            .expect("snapshot should be returned");

        assert_eq!(snapshot.thread_id, thread_id);
        assert_eq!(snapshot.runtime_agent_id, runtime.runtime_agent_id);
        assert_eq!(snapshot.turn_count, 0);
        assert!(!snapshot.messages.is_empty());
        assert_eq!(snapshot.messages[0].role, "system");
    }

    #[tokio::test]
    async fn create_runtime_agent_from_template_enables_approval_for_risky_tools() {
        use std::collections::HashMap;

        let temp_dir = tempdir().expect("temp dir should exist");
        let database_path = temp_dir.path().join("sqlite.db");
        let ctx = AppContext::init(Some(database_path.display().to_string()))
            .await
            .expect("app context init should succeed");

        ctx.upsert_provider(crate::LlmProviderRecord {
            id: crate::LlmProviderId::new(1),
            kind: crate::LlmProviderKind::OpenAiCompatible,
            display_name: "OpenAI".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: crate::SecretString::new("sk-test"),
            models: vec!["gpt-4.1".to_string()],
            default_model: "gpt-4.1".to_string(),
            is_default: true,
            extra_headers: HashMap::new(),
            secret_status: crate::ProviderSecretStatus::Ready,
        })
        .await
        .expect("provider should save");

        let default_template = ctx
            .get_default_agent_template()
            .await
            .expect("default agent should exist");

        let runtime = ctx
            .create_runtime_agent_from_template(&default_template.id, None)
            .await
            .expect("runtime agent should be created");

        let result = ctx.resolve_approval(
            &runtime.runtime_agent_id,
            uuid::Uuid::new_v4(),
            crate::ApprovalDecision::Approved,
            Some("desktop-user".to_string()),
        );

        assert!(
            matches!(result, Err(crate::AgentError::ApprovalFailed { .. })),
            "runtime agent should have an approval manager for risky tools",
        );
    }
}
