//! ArgusWing - Unified entry point for the ArgusWing application.
//!
//! This crate aggregates all managers and provides a unified API for:
//! - LLM Provider management (CRUD, testing)
//! - Agent Template management (CRUD)
//! - Session/Thread management
//! - Messaging and subscriptions
//! - Approval management
//!
//! ## Example
//!
//! ```rust,no_run
//! use argus_wing::ArgusWing;
//!
//! #[tokio::main]
//! async fn main() {
//!     let wing = ArgusWing::init(None).await.expect("Failed to initialize");
//!
//!     // List providers
//!     let providers = wing.list_providers().await.expect("Failed to list providers");
//!     println!("Providers: {:?}", providers);
//! }
//! ```

mod db;
mod resolver;

use std::sync::Arc;

use crate::db::{DatabaseTarget, default_trace_dir, ensure_parent_dir, resolve_database_target};

use argus_agent::CompactorManager;
use argus_approval::{ApprovalManager, ApprovalPolicy};
use argus_auth::AccountManager;
use argus_crypto::{Cipher, FileKeySource};
use argus_job::JobManager;
use argus_llm::ProviderManager;
use argus_protocol::{
    AgentId, AgentRecord, ArgusError, LlmProvider, LlmProviderId, LlmProviderRecord, ProviderId,
    ProviderTestResult, Result, RiskLevel, SessionId, ThreadEvent, ThreadId,
};
use argus_repository::traits::{
    AccountRepository, AgentRepository, LlmProviderRepository, SessionRepository, ThreadRepository,
};
use argus_repository::{connect, connect_path, migrate, ArgusSqlite};
use argus_session::{SessionManager, SessionSummary, ThreadSummary};
use argus_template::TemplateManager;
use argus_tool::ToolManager;
use sqlx::SqlitePool;
use tokio::sync::broadcast;

pub use resolver::ProviderManagerResolver;

/// Default agent display name for the ArgusWing template.
const DEFAULT_AGENT_DISPLAY_NAME: &str = "ArgusWing";

/// Unified entry point for the ArgusWing application.
///
/// This struct aggregates all managers and provides a unified API for:
/// - LLM Provider management
/// - Agent Template management
/// - Session/Thread management
/// - Messaging and subscriptions
/// - Approval management
pub struct ArgusWing {
    #[allow(dead_code)]
    pool: SqlitePool,
    provider_manager: Arc<ProviderManager>,
    template_manager: Arc<TemplateManager>,
    session_manager: Arc<SessionManager>,
    approval_manager: Arc<ApprovalManager>,
    tool_manager: Arc<ToolManager>,
    compactor_manager: Arc<CompactorManager>,
    #[allow(dead_code)]
    job_manager: Arc<JobManager>,
    pub account_manager: Arc<AccountManager>,
}

impl ArgusWing {
    /// Initialize ArgusWing with an optional database path.
    ///
    /// If no path is provided, defaults to `~/.arguswing/sqlite.db`.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The database cannot be connected
    /// - Migrations fail
    /// - The default template cannot be ensured
    pub async fn init(database_path: Option<&str>) -> Result<Arc<Self>> {
        let database_path = resolve_database_target(database_path)?;
        let pool = match &database_path {
            DatabaseTarget::Url(database_url) => connect(database_url).await,
            DatabaseTarget::Path(path) => {
                ensure_parent_dir(path)?;
                connect_path(path).await
            }
        }?;
        migrate(&pool).await?;

        // Create auth components first (needed for account management)
        let cipher = Arc::new(Cipher::new(FileKeySource::from_env_or_default()));
        let account_repo: Arc<dyn AccountRepository> = Arc::new(ArgusSqlite::new(pool.clone()));
        let account_manager = Arc::new(AccountManager::new(account_repo.clone(), cipher.clone()));

        // Create LLM provider repository and manager
        let llm_repository: Arc<dyn LlmProviderRepository> =
            Arc::new(ArgusSqlite::new(pool.clone()));
        let provider_manager = Arc::new(
            ProviderManager::new(llm_repository.clone()).with_auth(account_repo, cipher.clone()),
        );

        // Create template manager
        let arc_sqlite = Arc::new(ArgusSqlite::new(pool.clone()));
        let template_manager = Arc::new(TemplateManager::new(
            arc_sqlite.clone() as Arc<dyn AgentRepository>,
            arc_sqlite.clone(),
        ));
        template_manager.repair_placeholder_ids().await?;

        // Seed builtin agents from agents/ directory
        template_manager.seed_builtin_agents().await?;

        // Create tool manager
        let tool_manager = Arc::new(ToolManager::new());

        // Create compactor manager
        let compactor_manager = Arc::new(CompactorManager::with_defaults());

        // Create provider resolver wrapper FIRST
        let provider_resolver = Arc::new(ProviderManagerResolver::new(provider_manager.clone()));

        // Create job manager with all dependencies
        let job_manager = Arc::new(JobManager::new(
            template_manager.clone(),
            provider_resolver.clone(),
            tool_manager.clone(),
        ));

        // Create session manager
        let trace_dir = default_trace_dir();
        std::fs::create_dir_all(&trace_dir).ok();
        let session_manager = Arc::new(SessionManager::new(
            arc_sqlite.clone() as Arc<dyn SessionRepository>,
            arc_sqlite.clone() as Arc<dyn ThreadRepository>,
            Arc::clone(&llm_repository) as Arc<dyn LlmProviderRepository>,
            template_manager.clone(),
            provider_resolver,
            tool_manager.clone(),
            compactor_manager.clone(),
            trace_dir,
            job_manager.clone(),
        ));

        // Create approval manager
        let approval_manager = Arc::new(ApprovalManager::new(ApprovalPolicy::default()));

        Ok(Arc::new(Self {
            pool,
            provider_manager,
            template_manager,
            session_manager,
            approval_manager,
            tool_manager,
            compactor_manager,
            job_manager,
            account_manager,
        }))
    }

    /// Create a new ArgusWing with a pre-configured database pool.
    #[must_use]
    pub fn with_pool(pool: SqlitePool) -> Arc<Self> {
        // Create auth components first
        let cipher = Arc::new(Cipher::new(FileKeySource::from_env_or_default()));
        let account_repo: Arc<dyn AccountRepository> = Arc::new(ArgusSqlite::new(pool.clone()));
        let account_manager = Arc::new(AccountManager::new(account_repo.clone(), cipher.clone()));

        // Create LLM provider repository and manager
        let llm_repository: Arc<dyn LlmProviderRepository> =
            Arc::new(ArgusSqlite::new(pool.clone()));
        let provider_manager = Arc::new(
            ProviderManager::new(llm_repository.clone()).with_auth(account_repo, cipher.clone()),
        );
        let arc_sqlite = Arc::new(ArgusSqlite::new(pool.clone()));
        let template_manager = Arc::new(TemplateManager::new(
            arc_sqlite.clone() as Arc<dyn AgentRepository>,
            arc_sqlite.clone(),
        ));
        let tool_manager = Arc::new(ToolManager::new());
        let compactor_manager = Arc::new(CompactorManager::with_defaults());
        // Create provider resolver wrapper FIRST
        let provider_resolver = Arc::new(ProviderManagerResolver::new(provider_manager.clone()));

        // Create job manager with all dependencies
        let job_manager = Arc::new(JobManager::new(
            template_manager.clone(),
            provider_resolver.clone(),
            tool_manager.clone(),
        ));
        let trace_dir = default_trace_dir();
        std::fs::create_dir_all(&trace_dir).ok();
        let session_manager = Arc::new(SessionManager::new(
            arc_sqlite.clone() as Arc<dyn SessionRepository>,
            arc_sqlite.clone() as Arc<dyn ThreadRepository>,
            Arc::clone(&llm_repository) as Arc<dyn LlmProviderRepository>,
            template_manager.clone(),
            provider_resolver,
            tool_manager.clone(),
            compactor_manager.clone(),
            trace_dir,
            job_manager.clone(),
        ));
        let approval_manager = Arc::new(ApprovalManager::new(ApprovalPolicy::default()));

        Arc::new(Self {
            pool,
            provider_manager,
            template_manager,
            session_manager,
            approval_manager,
            tool_manager,
            compactor_manager,
            job_manager,
            account_manager,
        })
    }

    /// Get a reference to the tool manager.
    #[must_use]
    pub fn tool_manager(&self) -> &Arc<ToolManager> {
        &self.tool_manager
    }

    /// Register default tools (shell, read, grep, glob, http, write, list, patch) with the tool manager.
    pub async fn register_default_tools(&self) -> Result<()> {
        use argus_tool::{
            ApplyPatchTool, ChromeTool, GlobTool, GrepTool, HttpTool, ListDirTool, ReadTool,
            ShellTool, WriteFileTool,
        };

        self.tool_manager.register(Arc::new(ShellTool::new()));
        self.tool_manager.register(Arc::new(ReadTool::new()));
        self.tool_manager.register(Arc::new(GrepTool::new()));
        self.tool_manager.register(Arc::new(GlobTool::new()));
        self.tool_manager.register(Arc::new(HttpTool::new()));
        self.tool_manager.register(Arc::new(WriteFileTool::new()));
        self.tool_manager.register(Arc::new(ListDirTool::new()));
        self.tool_manager.register(Arc::new(ApplyPatchTool::new()));
        self.tool_manager.register(Arc::new(ChromeTool::new()));

        Ok(())
    }

    /// Get a reference to the approval manager.
    #[must_use]
    pub fn approval_manager(&self) -> &Arc<ApprovalManager> {
        &self.approval_manager
    }

    /// Get a reference to the compactor manager.
    #[must_use]
    pub fn compactor_manager(&self) -> &Arc<CompactorManager> {
        &self.compactor_manager
    }

    /// Get a reference to the account manager.
    #[must_use]
    pub fn account_manager(&self) -> &Arc<AccountManager> {
        &self.account_manager
    }

    // =========================================================================
    // Provider CRUD API
    // =========================================================================

    /// List all provider records.
    pub async fn list_providers(&self) -> Result<Vec<LlmProviderRecord>> {
        self.provider_manager.list_providers().await
    }

    /// Get a provider record by ID.
    pub async fn get_provider_record(&self, id: LlmProviderId) -> Result<LlmProviderRecord> {
        self.provider_manager.get_provider_record(&id).await
    }

    /// Upsert a provider record.
    pub async fn upsert_provider(&self, record: LlmProviderRecord) -> Result<LlmProviderId> {
        self.provider_manager.upsert_provider(record).await
    }

    /// Delete a provider by ID.
    pub async fn delete_provider(&self, id: LlmProviderId) -> Result<bool> {
        self.provider_manager.delete_provider(&id).await
    }

    /// Set the default provider.
    pub async fn set_default_provider(&self, id: LlmProviderId) -> Result<()> {
        self.provider_manager.set_default_provider(&id).await
    }

    /// Test a provider connection.
    pub async fn test_provider_connection(
        &self,
        id: LlmProviderId,
        model: &str,
    ) -> Result<ProviderTestResult> {
        self.provider_manager
            .test_provider_connection(&id, model)
            .await
    }

    /// Test a provider record (without saving).
    pub async fn test_provider_record(
        &self,
        record: LlmProviderRecord,
        model: &str,
    ) -> Result<ProviderTestResult> {
        self.provider_manager
            .test_provider_record(record, model)
            .await
    }

    /// Get the default provider record.
    pub async fn get_default_provider_record(&self) -> Result<LlmProviderRecord> {
        self.provider_manager.get_default_provider_record().await
    }

    /// Get a provider instance by ID (for calling methods like context_window).
    pub async fn get_provider(&self, id: LlmProviderId) -> Result<Arc<dyn LlmProvider>> {
        self.provider_manager.get_provider(&id).await
    }

    // =========================================================================
    // Template CRUD API
    // =========================================================================

    /// List all templates.
    pub async fn list_templates(&self) -> Result<Vec<AgentRecord>> {
        self.template_manager.list().await
    }

    /// Get a template by ID.
    pub async fn get_template(&self, id: AgentId) -> Result<Option<AgentRecord>> {
        self.template_manager.get(id).await
    }

    /// Upsert a template.
    pub async fn upsert_template(&self, record: AgentRecord) -> Result<AgentId> {
        self.template_manager.upsert(record).await
    }

    /// Delete a template by ID.
    pub async fn delete_template(&self, id: AgentId) -> Result<()> {
        self.template_manager.delete(id).await
    }

    /// List all subagents of a given parent agent.
    pub async fn list_subagents(&self, parent_id: AgentId) -> Result<Vec<AgentRecord>> {
        self.template_manager.list_subagents(parent_id).await
    }

    /// Add a subagent to a parent agent (set child's parent_agent_id).
    pub async fn add_subagent(&self, parent_id: AgentId, child_id: AgentId) -> Result<()> {
        self.template_manager
            .add_subagent(parent_id, child_id)
            .await
    }

    /// Remove a subagent from its parent (clear child's parent_agent_id).
    pub async fn remove_subagent(&self, parent_id: AgentId, child_id: AgentId) -> Result<()> {
        self.template_manager
            .remove_subagent(parent_id, child_id)
            .await
    }

    /// Get the default agent template.
    ///
    /// Returns the template record for the default "arguswing" agent if it exists.
    /// This template contains the system prompt and default settings.
    ///
    /// Returns `Ok(None)` if the default template does not exist in the database.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn get_default_template(&self) -> Result<Option<AgentRecord>> {
        self.template_manager
            .find_by_display_name(DEFAULT_AGENT_DISPLAY_NAME)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: format!("Failed to fetch default template: {}", e),
            })
    }

    // =========================================================================
    // Session Management API
    // =========================================================================

    /// List all sessions.
    pub async fn list_sessions(&self) -> Result<Vec<SessionSummary>> {
        self.session_manager.list_sessions().await
    }

    /// Create a new session.
    pub async fn create_session(&self, name: &str) -> Result<SessionId> {
        self.session_manager.create(name.to_string()).await
    }

    /// Create a session with approval policy.
    ///
    /// Creates a new session and thread with the specified approval configuration.
    ///
    /// # Arguments
    /// * `name` - Session name
    /// * `approval_tools` - List of tool names that require approval
    /// * `auto_approve` - Whether to auto-approve tools
    ///
    /// # Returns
    /// Tuple of (session_id, thread_id) if successful
    ///
    /// # Errors
    /// Returns an error if:
    /// - Default template 'ArgusWing' is not found
    /// - Session or thread creation fails
    pub async fn create_session_with_approval(
        &self,
        name: &str,
        approval_tools: Vec<String>,
        auto_approve: bool,
    ) -> Result<(SessionId, ThreadId)> {
        let session_id = self.create_session(name).await?;

        // Get default template
        let template =
            self.get_default_template()
                .await?
                .ok_or_else(|| ArgusError::ApprovalError {
                    reason: "Default template 'ArgusWing' not found".to_string(),
                })?;

        // Configure approval policy if needed
        if !approval_tools.is_empty() {
            let policy = ApprovalPolicy {
                require_approval: approval_tools.clone(),
                auto_approve,
                ..Default::default()
            };
            self.approval_manager.update_policy(policy);
        }

        // Create thread
        let thread_id = self
            .create_thread(session_id, template.id, None, None)
            .await?;

        Ok((session_id, thread_id))
    }

    /// Load a session into memory.
    pub async fn load_session(&self, session_id: SessionId) -> Result<Arc<argus_session::Session>> {
        self.session_manager.load(session_id).await
    }

    /// Delete a session.
    pub async fn delete_session(&self, session_id: SessionId) -> Result<()> {
        self.session_manager.delete(session_id).await
    }

    // =========================================================================
    // Thread Management API
    // =========================================================================

    /// Create a new thread in a session.
    pub async fn create_thread(
        &self,
        session_id: SessionId,
        template_id: AgentId,
        provider_id: Option<ProviderId>,
        model_override: Option<&str>,
    ) -> Result<ThreadId> {
        self.session_manager
            .create_thread(session_id, template_id, provider_id, model_override)
            .await
    }

    /// List threads in a session.
    pub async fn list_threads(&self, session_id: SessionId) -> Result<Vec<ThreadSummary>> {
        self.session_manager.list_threads(session_id).await
    }

    /// Delete a thread from a session.
    pub async fn delete_thread(&self, session_id: SessionId, thread_id: ThreadId) -> Result<()> {
        self.session_manager
            .delete_thread(session_id, &thread_id)
            .await
    }

    /// Rename a persisted session.
    pub async fn rename_session(&self, session_id: SessionId, name: String) -> Result<()> {
        self.session_manager.rename_session(session_id, name).await
    }

    /// Rename a persisted thread title.
    pub async fn rename_thread(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
        title: String,
    ) -> Result<()> {
        self.session_manager
            .rename_thread(session_id, &thread_id, title)
            .await
    }

    /// Update the bound provider/model for an existing thread.
    pub async fn update_thread_model(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
        provider_id: ProviderId,
        model: &str,
    ) -> Result<(ProviderId, String)> {
        self.session_manager
            .update_thread_model(session_id, &thread_id, provider_id, model)
            .await
    }

    // =========================================================================
    // Messaging API
    // =========================================================================

    /// Send a message to a thread.
    pub async fn send_message(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
        message: String,
    ) -> Result<()> {
        self.session_manager
            .send_message(session_id, &thread_id, message)
            .await
    }

    /// Get the thread message history, recovering persisted turn summaries when needed.
    pub async fn get_thread_messages(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Result<Vec<argus_protocol::llm::ChatMessage>> {
        self.session_manager
            .get_thread_messages(session_id, &thread_id)
            .await
    }

    /// Activate a persisted thread into live memory so it can continue chatting.
    pub async fn activate_thread(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Result<(AgentId, Option<ProviderId>, Option<String>)> {
        self.session_manager
            .activate_thread(session_id, &thread_id)
            .await
    }

    /// Subscribe to thread events.
    pub async fn subscribe(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Option<broadcast::Receiver<ThreadEvent>> {
        self.session_manager.subscribe(session_id, &thread_id).await
    }

    // =========================================================================
    // Approval API
    // =========================================================================

    /// List pending approval requests.
    #[must_use]
    pub fn list_pending_approvals(&self) -> Vec<argus_protocol::ApprovalRequest> {
        self.approval_manager.list_pending()
    }

    /// Resolve an approval request.
    pub fn resolve_approval(
        &self,
        request_id: uuid::Uuid,
        decision: argus_protocol::ApprovalDecision,
        resolved_by: Option<String>,
    ) -> Result<argus_protocol::ApprovalResponse> {
        self.approval_manager
            .resolve(request_id, decision, resolved_by)
            .map_err(|e| ArgusError::ApprovalError {
                reason: e.to_string(),
            })
    }

    // =========================================================================
    // Tool API
    // =========================================================================

    /// List all available tools with their metadata.
    pub async fn list_tools(&self) -> Vec<ToolInfo> {
        let definitions = self.tool_manager.list_definitions();
        definitions
            .into_iter()
            .map(|def| ToolInfo {
                name: def.name.clone(),
                description: def.description.clone(),
                risk_level: self.tool_manager.get_risk_level(&def.name),
                parameters: def.parameters,
            })
            .collect()
    }
}

// =========================================================================
// Helper Types
// =========================================================================

/// Tool information for frontend display.
pub struct ToolInfo {
    pub name: String,
    pub description: String,
    pub risk_level: RiskLevel,
    pub parameters: serde_json::Value,
}

#[cfg(test)]
mod tests {
    use super::*;
    use argus_protocol::{AgentType, ThinkingConfig};

    fn make_test_wing() -> Arc<ArgusWing> {
        let pool = SqlitePool::connect_lazy("sqlite::memory:")
            .expect("lazy sqlite pool should build for tests");
        ArgusWing::with_pool(pool)
    }

    #[tokio::test]
    async fn register_default_tools_includes_chrome() {
        let wing = make_test_wing();
        wing.register_default_tools();
        assert!(wing.tool_manager().get("chrome").is_some());
    }

    #[tokio::test]
    async fn init_creates_argus_wing_with_default_database() {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let database_path = temp_dir.path().join("sqlite.db");

        let wing = ArgusWing::init(Some(&database_path.display().to_string()))
            .await
            .expect("ArgusWing should initialize");

        let providers = wing
            .list_providers()
            .await
            .expect("provider list should succeed");
        // A default provider is created by migration
        assert_eq!(providers.len(), 1);
        assert!(providers[0].is_default);
    }

    #[tokio::test]
    async fn get_default_template_returns_template() {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let database_path = temp_dir.path().join("test.sqlite");

        let wing = ArgusWing::init(Some(&database_path.display().to_string()))
            .await
            .expect("ArgusWing should initialize");

        // Create the default template first
        let default_template = AgentRecord {
            id: AgentId::new(0), // Placeholder ID, will be auto-generated
            display_name: DEFAULT_AGENT_DISPLAY_NAME.to_string(),
            description: "Default assistant for ArgusWing".to_string(),
            version: "0.1.0".to_string(),
            provider_id: None,
            model_id: None,
            system_prompt: "You are ArgusWing, a helpful AI assistant.".to_string(),
            tool_names: vec!["shell".to_string(), "read".to_string()],
            max_tokens: None,
            temperature: None,
            thinking_config: Some(ThinkingConfig::enabled()),
            parent_agent_id: None,
            agent_type: AgentType::Standard,
        };
        wing.upsert_template(default_template)
            .await
            .expect("should upsert default template");

        // Test get_default_template
        let template = wing
            .get_default_template()
            .await
            .expect("should get default template");

        assert!(template.is_some());
        let template = template.unwrap();
        assert_eq!(template.display_name, DEFAULT_AGENT_DISPLAY_NAME);
        assert!(!template.system_prompt.is_empty());
        assert_eq!(template.display_name, "ArgusWing");
    }

    #[tokio::test]
    async fn upsert_template_with_placeholder_id_returns_real_id() {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let database_path = temp_dir.path().join("test.sqlite");

        let wing = ArgusWing::init(Some(&database_path.display().to_string()))
            .await
            .expect("ArgusWing should initialize");

        let new_template = AgentRecord {
            id: AgentId::new(0),
            display_name: "Real ID Agent".to_string(),
            description: "Should receive a database-generated id".to_string(),
            version: "1.0.0".to_string(),
            provider_id: None,
            model_id: None,
            system_prompt: "You are a test agent.".to_string(),
            tool_names: vec![],
            max_tokens: None,
            temperature: None,
            thinking_config: Some(ThinkingConfig::enabled()),
            parent_agent_id: None,
            agent_type: AgentType::Standard,
        };

        let template_id = wing
            .upsert_template(new_template)
            .await
            .expect("template should upsert");

        assert_ne!(
            template_id.inner(),
            0,
            "new templates should not keep placeholder id 0"
        );

        let stored = wing
            .get_template(template_id)
            .await
            .expect("template lookup should succeed")
            .expect("template should exist");

        assert_eq!(stored.id, template_id);
        assert_eq!(stored.display_name, "Real ID Agent");
    }

    #[tokio::test]
    async fn init_repairs_legacy_placeholder_agent_ids() {
        use argus_repository::ArgusSqlite;

        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let database_path = temp_dir.path().join("test.sqlite");

        let wing = ArgusWing::init(Some(&database_path.display().to_string()))
            .await
            .expect("ArgusWing should initialize");

        // Insert a legacy placeholder agent for the repair test
        let db = ArgusSqlite::new(wing.pool.clone());
        db.insert_legacy_agent_for_test()
            .await
            .expect("legacy zero-id agent should insert");

        drop(wing);

        let repaired = ArgusWing::init(Some(&database_path.display().to_string()))
            .await
            .expect("ArgusWing should re-initialize and repair legacy data");

        let templates = repaired
            .list_templates()
            .await
            .expect("template listing should succeed");

        assert!(templates.iter().all(|template| template.id.inner() != 0));
        assert!(
            templates
                .iter()
                .any(|template| template.display_name == "Legacy Zero Agent")
        );
    }

    #[tokio::test]
    async fn delete_template_reports_references_before_hitting_foreign_key_constraint() {
        use argus_protocol::LlmProviderRecord;
        use std::collections::HashMap;
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let database_path = temp_dir.path().join("test.sqlite");

        let wing = ArgusWing::init(Some(&database_path.display().to_string()))
            .await
            .expect("ArgusWing should initialize");

        let provider_record = LlmProviderRecord {
            id: argus_protocol::LlmProviderId::new(1),
            display_name: "test-provider".to_string(),
            kind: argus_protocol::LlmProviderKind::OpenAiCompatible,
            base_url: "http://localhost:11434/v1".to_string(),
            api_key: argus_protocol::SecretString::new("test-key"),
            models: vec!["gpt-4".to_string()],
            model_config: HashMap::new(),
            default_model: "gpt-4".to_string(),
            is_default: true,
            extra_headers: HashMap::new(),
            secret_status: argus_protocol::ProviderSecretStatus::Ready,
            meta_data: HashMap::new(),
        };

        let provider_id = wing
            .upsert_provider(provider_record)
            .await
            .expect("provider should upsert");

        let template_id = wing
            .upsert_template(AgentRecord {
                id: AgentId::new(0),
                display_name: "Delete Guard Agent".to_string(),
                description: "Used by an existing thread".to_string(),
                version: "1.0.0".to_string(),
                provider_id: Some(argus_protocol::ProviderId::new(provider_id.into_inner())),
                model_id: None,
                system_prompt: "You are a test agent.".to_string(),
                tool_names: vec![],
                max_tokens: None,
                temperature: None,
                thinking_config: Some(ThinkingConfig::enabled()),
                parent_agent_id: None,
                agent_type: AgentType::Standard,
            })
            .await
            .expect("template should upsert");

        let session_id = wing
            .create_session("delete-guard-session")
            .await
            .expect("session should create");

        wing.create_thread(
            session_id,
            template_id,
            Some(argus_protocol::ProviderId::new(provider_id.into_inner())),
            None,
        )
        .await
        .expect("thread should create");

        let error = wing
            .delete_template(template_id)
            .await
            .expect_err("template in use should not delete");

        let message = error.to_string();
        assert!(
            message.contains("无法删除智能体"),
            "unexpected error: {message}"
        );
        assert!(
            message.contains("1 个会话线程"),
            "unexpected error: {message}"
        );
    }

    #[tokio::test]
    async fn create_session_with_approval_configures_policy() {
        use argus_protocol::LlmProviderRecord;
        use std::collections::HashMap;
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let database_path = temp_dir.path().join("test.sqlite");

        let wing = ArgusWing::init(Some(&database_path.display().to_string()))
            .await
            .expect("ArgusWing should initialize");

        // Create a test provider first
        let provider_record = LlmProviderRecord {
            id: argus_protocol::LlmProviderId::new(1),
            display_name: "test-provider".to_string(),
            kind: argus_protocol::LlmProviderKind::OpenAiCompatible,
            base_url: "http://localhost:11434/v1".to_string(),
            api_key: argus_protocol::SecretString::new("test-key"),
            models: vec!["gpt-4".to_string()],
            model_config: HashMap::new(),
            default_model: "gpt-4".to_string(),
            is_default: true,
            extra_headers: HashMap::new(),
            secret_status: argus_protocol::ProviderSecretStatus::Ready,
            meta_data: HashMap::new(),
        };

        let provider_id = wing
            .upsert_provider(provider_record.clone())
            .await
            .expect("provider should upsert");

        wing.set_default_provider(provider_id)
            .await
            .expect("should set default provider");

        // Create the default template
        let default_template = AgentRecord {
            id: AgentId::new(0),
            display_name: DEFAULT_AGENT_DISPLAY_NAME.to_string(),
            description: "Default assistant for ArgusWing".to_string(),
            version: "0.1.0".to_string(),
            provider_id: Some(argus_protocol::ProviderId::new(provider_id.into_inner())),
            model_id: None,
            system_prompt: "You are ArgusWing, a helpful AI assistant.".to_string(),
            tool_names: vec!["shell".to_string(), "read".to_string()],
            max_tokens: None,
            temperature: None,
            thinking_config: Some(ThinkingConfig::enabled()),
            parent_agent_id: None,
            agent_type: AgentType::Standard,
        };
        wing.upsert_template(default_template)
            .await
            .expect("should upsert default template");

        let (session_id, _thread_id) = wing
            .create_session_with_approval("test-session", vec!["shell".to_string()], false)
            .await
            .expect("session with approval should create");

        // Verify session was created
        let sessions = wing.list_sessions().await.expect("should list sessions");
        assert!(!sessions.is_empty());

        // Verify thread was created
        let threads = wing
            .list_threads(session_id)
            .await
            .expect("should list threads");
        assert_eq!(threads.len(), 1);
    }

    #[tokio::test]
    async fn create_thread_uses_default_provider_when_template_has_none() {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let database_path = temp_dir.path().join("test.sqlite");

        let wing = ArgusWing::init(Some(&database_path.display().to_string()))
            .await
            .expect("ArgusWing should initialize");

        let template_id = wing
            .upsert_template(AgentRecord {
                id: AgentId::new(0),
                display_name: "No Provider Agent".to_string(),
                description: "Used to verify default provider fallback".to_string(),
                version: "1.0.0".to_string(),
                provider_id: None,
                model_id: None,
                system_prompt: "You are a fallback agent.".to_string(),
                tool_names: vec![],
                max_tokens: None,
                temperature: None,
                thinking_config: Some(ThinkingConfig::enabled()),
                parent_agent_id: None,
                agent_type: AgentType::Standard,
            })
            .await
            .expect("template should upsert");

        let session_id = wing
            .create_session("fallback-provider-session")
            .await
            .expect("session should create");

        wing.create_thread(session_id, template_id, None, None)
            .await
            .expect("thread should create using the default provider fallback");
    }

    #[tokio::test]
    async fn create_thread_pins_agent_default_model_for_later_activation() {
        use argus_protocol::LlmProviderRecord;
        use std::collections::HashMap;

        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let database_path = temp_dir.path().join("test.sqlite");

        let wing = ArgusWing::init(Some(&database_path.display().to_string()))
            .await
            .expect("ArgusWing should initialize");

        let provider_record = LlmProviderRecord {
            id: argus_protocol::LlmProviderId::new(1),
            display_name: "test-provider".to_string(),
            kind: argus_protocol::LlmProviderKind::OpenAiCompatible,
            base_url: "http://localhost:11434/v1".to_string(),
            api_key: argus_protocol::SecretString::new("test-key"),
            models: vec!["gpt-4o-mini".to_string(), "gpt-5".to_string()],
            model_config: HashMap::new(),
            default_model: "gpt-4o-mini".to_string(),
            is_default: true,
            extra_headers: HashMap::new(),
            secret_status: argus_protocol::ProviderSecretStatus::Ready,
            meta_data: HashMap::new(),
        };

        let provider_id = wing
            .upsert_provider(provider_record)
            .await
            .expect("provider should upsert");

        wing.set_default_provider(provider_id)
            .await
            .expect("should set default provider");

        let template_id = wing
            .upsert_template(AgentRecord {
                id: AgentId::new(0),
                display_name: "Pinned Model Agent".to_string(),
                description: "Uses a non-default model".to_string(),
                version: "1.0.0".to_string(),
                provider_id: Some(argus_protocol::ProviderId::new(provider_id.into_inner())),
                model_id: Some("gpt-5".to_string()),
                system_prompt: "You are a pinned-model agent.".to_string(),
                tool_names: vec![],
                max_tokens: None,
                temperature: None,
                thinking_config: Some(ThinkingConfig::enabled()),
                parent_agent_id: None,
                agent_type: AgentType::Standard,
            })
            .await
            .expect("template should upsert");

        let session_id = wing
            .create_session("pinned-model-session")
            .await
            .expect("session should create");

        let thread_id = wing
            .create_thread(session_id, template_id, None, None)
            .await
            .expect("thread should create");

        let (activated_template_id, effective_provider_id, effective_model) = wing
            .activate_thread(session_id, thread_id)
            .await
            .expect("thread should activate");

        assert_eq!(activated_template_id, template_id);
        assert_eq!(
            effective_provider_id,
            Some(argus_protocol::ProviderId::new(provider_id.into_inner()))
        );
        assert_eq!(effective_model.as_deref(), Some("gpt-5"));
    }

    #[tokio::test]
    async fn update_thread_model_rebinds_existing_thread() {
        use argus_protocol::LlmProviderRecord;
        use std::collections::HashMap;

        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let database_path = temp_dir.path().join("test.sqlite");

        let wing = ArgusWing::init(Some(&database_path.display().to_string()))
            .await
            .expect("ArgusWing should initialize");

        let provider_record = LlmProviderRecord {
            id: argus_protocol::LlmProviderId::new(1),
            display_name: "test-provider".to_string(),
            kind: argus_protocol::LlmProviderKind::OpenAiCompatible,
            base_url: "http://localhost:11434/v1".to_string(),
            api_key: argus_protocol::SecretString::new("test-key"),
            models: vec!["gpt-4o-mini".to_string(), "gpt-5".to_string()],
            model_config: HashMap::new(),
            default_model: "gpt-4o-mini".to_string(),
            is_default: true,
            extra_headers: HashMap::new(),
            secret_status: argus_protocol::ProviderSecretStatus::Ready,
            meta_data: HashMap::new(),
        };

        let provider_id = wing
            .upsert_provider(provider_record)
            .await
            .expect("provider should upsert");

        wing.set_default_provider(provider_id)
            .await
            .expect("should set default provider");

        let template_id = wing
            .upsert_template(AgentRecord {
                id: AgentId::new(0),
                display_name: "Mutable Model Agent".to_string(),
                description: "Lets a running thread swap models".to_string(),
                version: "1.0.0".to_string(),
                provider_id: Some(argus_protocol::ProviderId::new(provider_id.into_inner())),
                model_id: None,
                system_prompt: "You are a mutable-model agent.".to_string(),
                tool_names: vec![],
                max_tokens: None,
                temperature: None,
                thinking_config: Some(ThinkingConfig::enabled()),
                parent_agent_id: None,
                agent_type: AgentType::Standard,
            })
            .await
            .expect("template should upsert");

        let session_id = wing
            .create_session("update-model-session")
            .await
            .expect("session should create");

        let thread_id = wing
            .create_thread(session_id, template_id, None, None)
            .await
            .expect("thread should create");

        let (initial_template_id, initial_provider_id, initial_model) = wing
            .activate_thread(session_id, thread_id)
            .await
            .expect("thread should activate");
        assert_eq!(initial_template_id, template_id);
        assert_eq!(
            initial_provider_id,
            Some(argus_protocol::ProviderId::new(provider_id.into_inner()))
        );
        assert_eq!(initial_model.as_deref(), Some("gpt-4o-mini"));

        let (updated_provider_id, updated_model) = wing
            .update_thread_model(
                session_id,
                thread_id,
                argus_protocol::ProviderId::new(provider_id.into_inner()),
                "gpt-5",
            )
            .await
            .expect("thread model should update");

        assert_eq!(
            updated_provider_id,
            argus_protocol::ProviderId::new(provider_id.into_inner())
        );
        assert_eq!(updated_model, "gpt-5");

        let session = wing
            .load_session(session_id)
            .await
            .expect("session should load");
        let thread = session
            .get_thread(&thread_id)
            .expect("thread should stay loaded");
        let thread = thread.read().await;
        assert_eq!(thread.provider().model_name(), "gpt-5");
    }

    #[tokio::test]
    async fn register_default_tools_registers_builtin_tool_ids() {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let database_path = temp_dir.path().join("sqlite.db");

        let wing = ArgusWing::init(Some(&database_path.display().to_string()))
            .await
            .expect("ArgusWing should initialize");

        wing.register_default_tools()
            .await
            .expect("default tool registration should succeed");

        let mut tool_ids = wing.tool_manager().list_ids();
        tool_ids.sort();

        for expected_tool in [
            "apply_patch",
            "glob",
            "grep",
            "http",
            "list_dir",
            "read",
            "shell",
            "write_file",
        ] {
            assert!(
                tool_ids.iter().any(|tool_id| tool_id == expected_tool),
                "missing expected tool: {expected_tool}"
            );
        }
    }
}
