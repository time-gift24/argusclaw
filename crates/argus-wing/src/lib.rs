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

use std::sync::Arc;

use argus_approval::{ApprovalManager, ApprovalPolicy};
use argus_llm::ProviderManager;
use argus_protocol::{
    AgentId, AgentRecord, ArgusError, LlmProvider, LlmProviderId, LlmProviderRecord,
    ProviderId, ProviderTestResult, Result, SessionId, ThreadEvent, ThreadId,
};
use argus_repository::{connect, connect_path, migrate, ArgusSqlite};
use argus_session::{ProviderResolver, SessionManager, SessionSummary, ThreadSummary};
use argus_template::TemplateManager;
use argus_thread::CompactorManager;
use argus_tool::ToolManager;
use sqlx::SqlitePool;
use tokio::sync::broadcast;

/// Default agent display name for the ArgusWing template.
const DEFAULT_AGENT_DISPLAY_NAME: &str = "ArgusWing";

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

#[async_trait::async_trait]
impl ProviderResolver for ProviderManagerResolver {
    async fn resolve(&self, id: ProviderId) -> Result<Arc<dyn LlmProvider>> {
        let provider_id = LlmProviderId::new(id.inner());
        self.provider_manager.get_provider(&provider_id).await
    }

    async fn default_provider(&self) -> Result<Arc<dyn LlmProvider>> {
        self.provider_manager.get_default_provider().await
    }
}

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

        // Create LLM provider repository and manager
        let llm_repository = Arc::new(ArgusSqlite::new(pool.clone()));
        let provider_manager = Arc::new(ProviderManager::new(llm_repository));

        // Create template manager
        let template_manager = Arc::new(TemplateManager::new(pool.clone()));

        // Create tool manager
        let tool_manager = Arc::new(ToolManager::new());

        // Create compactor manager
        let compactor_manager = Arc::new(CompactorManager::with_defaults());

        // Create provider resolver wrapper
        let provider_resolver = Arc::new(ProviderManagerResolver::new(provider_manager.clone()));

        // Create session manager
        let session_manager = Arc::new(SessionManager::new(
            pool.clone(),
            template_manager.clone(),
            provider_resolver,
            tool_manager.clone(),
            compactor_manager.clone(),
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
        }))
    }

    /// Create a new ArgusWing with a pre-configured database pool.
    #[must_use]
    pub fn with_pool(pool: SqlitePool) -> Arc<Self> {
        let llm_repository = Arc::new(ArgusSqlite::new(pool.clone()));
        let provider_manager = Arc::new(ProviderManager::new(llm_repository));
        let template_manager = Arc::new(TemplateManager::new(pool.clone()));
        let tool_manager = Arc::new(ToolManager::new());
        let compactor_manager = Arc::new(CompactorManager::with_defaults());
        let provider_resolver = Arc::new(ProviderManagerResolver::new(provider_manager.clone()));
        let session_manager = Arc::new(SessionManager::new(
            pool.clone(),
            template_manager.clone(),
            provider_resolver,
            tool_manager.clone(),
            compactor_manager.clone(),
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
        })
    }

    /// Get a reference to the tool manager.
    #[must_use]
    pub fn tool_manager(&self) -> &Arc<ToolManager> {
        &self.tool_manager
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
        self.provider_manager.test_provider_record(record, model).await
    }

    /// Get the default provider record.
    pub async fn get_default_provider_record(&self) -> Result<LlmProviderRecord> {
        self.provider_manager.get_default_provider_record().await
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

    /// Create a session with custom approval policy.
    ///
    /// Creates a new session (thread) with the specified approval policy.
    /// The approval policy controls which tools require explicit approval.
    ///
    /// # Arguments
    /// * `template_id` - The agent template to use
    /// * `approval_policy` - Custom approval policy configuration
    ///
    /// # Returns
    /// The session ID if successful
    ///
    /// # Note
    /// Currently, the approval policy is not persisted to the database.
    /// This will be addressed in a future update.
    pub async fn create_session_with_approval(
        &self,
        template_id: &AgentId,
        _approval_policy: ApprovalPolicy,
    ) -> Result<SessionId> {
        // Get template
        let template = self
            .get_template(*template_id)
            .await?
            .ok_or_else(|| ArgusError::DatabaseError {
                reason: format!("Template {} not found", template_id.inner()),
            })?;

        // Create session with template name as session name
        let session_id = self
            .session_manager
            .create(template.display_name.clone())
            .await?;

        // TODO: Store approval policy with session
        // For now, create thread without approval policy storage
        let _thread = self
            .session_manager
            .create_thread(session_id, template.id, None)
            .await?;

        Ok(session_id)
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
    ) -> Result<ThreadId> {
        self.session_manager
            .create_thread(session_id, template_id, provider_id)
            .await
    }

    /// List threads in a session.
    pub async fn list_threads(&self, session_id: SessionId) -> Result<Vec<ThreadSummary>> {
        self.session_manager.list_threads(session_id).await
    }

    /// Delete a thread from a session.
    pub async fn delete_thread(&self, session_id: SessionId, thread_id: ThreadId) -> Result<()> {
        self.session_manager.delete_thread(session_id, &thread_id).await
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
}

// =========================================================================
// Helper Types and Functions
// =========================================================================

enum DatabaseTarget {
    Url(String),
    Path(std::path::PathBuf),
}

fn resolve_database_target(configured: Option<&str>) -> Result<DatabaseTarget> {
    let configured = configured
        .map(|s| s.to_string())
        .unwrap_or_else(default_database_target);

    if configured.starts_with("sqlite:") {
        return Ok(DatabaseTarget::Url(configured));
    }

    Ok(DatabaseTarget::Path(expand_home_path(&configured)?))
}

fn default_database_target() -> String {
    std::env::var("DATABASE_URL").unwrap_or_else(|_| "~/.arguswing/sqlite.db".to_string())
}

fn expand_home_path(path: &str) -> Result<std::path::PathBuf> {
    if let Some(relative_path) = path.strip_prefix("~/") {
        let home_dir = dirs::home_dir().ok_or_else(|| ArgusError::DatabaseError {
            reason: "Cannot determine home directory".to_string(),
        })?;
        return Ok(home_dir.join(relative_path));
    }

    Ok(std::path::PathBuf::from(path))
}

fn ensure_parent_dir(path: &std::path::Path) -> Result<()> {
    let parent = path.parent().ok_or_else(|| ArgusError::DatabaseError {
        reason: format!("Invalid database path: {}", path.display()),
    })?;
    std::fs::create_dir_all(parent).map_err(|e| ArgusError::DatabaseError {
        reason: format!("Cannot create database directory: {}", e),
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(providers.is_empty());
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
            system_prompt: "You are ArgusWing, a helpful AI assistant.".to_string(),
            tool_names: vec!["shell".to_string(), "read".to_string()],
            max_tokens: None,
            temperature: None,
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
    async fn create_session_with_custom_approval() {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let database_path = temp_dir.path().join("test.sqlite");

        let wing = ArgusWing::init(Some(&database_path.display().to_string()))
            .await
            .expect("ArgusWing should initialize");

        // Create a mock provider for testing
        use std::collections::HashMap;
        let provider_record = LlmProviderRecord {
            id: LlmProviderId::new(1),
            kind: argus_protocol::LlmProviderKind::OpenAiCompatible,
            display_name: "Test Provider".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            api_key: argus_protocol::SecretString::new("test-key"),
            models: vec!["gpt-4".to_string()],
            default_model: "gpt-4".to_string(),
            is_default: true,
            extra_headers: HashMap::new(),
            secret_status: argus_protocol::ProviderSecretStatus::Ready,
        };
        wing.upsert_provider(provider_record.clone())
            .await
            .expect("should upsert provider");
        wing.set_default_provider(provider_record.id)
            .await
            .expect("should set default provider");

        // Create the default template
        let default_template = AgentRecord {
            id: AgentId::new(0), // Placeholder ID, will be auto-generated
            display_name: DEFAULT_AGENT_DISPLAY_NAME.to_string(),
            description: "Default assistant for ArgusWing".to_string(),
            version: "0.1.0".to_string(),
            provider_id: Some(argus_protocol::ProviderId::new(1)),
            system_prompt: "You are ArgusWing, a helpful AI assistant.".to_string(),
            tool_names: vec!["shell".to_string(), "read".to_string()],
            max_tokens: None,
            temperature: None,
        };
        wing.upsert_template(default_template)
            .await
            .expect("should upsert default template");

        let template = wing
            .get_default_template()
            .await
            .expect("should get default template")
            .expect("default template should exist");

        let approval_policy = ApprovalPolicy {
            require_approval: vec!["shell".to_string()],
            timeout_secs: 300,
            auto_approve_autonomous: false,
            auto_approve: false,
        };

        let session_id = wing
            .create_session_with_approval(&template.id, approval_policy)
            .await
            .expect("should create session with approval");

        // Verify session exists
        let sessions = wing
            .list_sessions()
            .await
            .expect("should list sessions");
        assert!(sessions.len() >= 1);
        assert!(sessions.iter().any(|s| s.id == session_id));
    }
}
