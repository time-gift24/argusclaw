use std::sync::Arc;
use std::{env, path::Path, path::PathBuf};

use crate::agents::AgentManager;
#[cfg(feature = "dev")]
use crate::db::llm::{LlmProviderId, LlmProviderRecord};
use crate::db::sqlite::{SqliteLlmProviderRepository, connect, connect_path, migrate};
use crate::error::AgentError;
use crate::llm::LLMManager;
#[cfg(feature = "dev")]
use crate::llm::LlmEventStream;
use crate::tool::ToolManager;

#[derive(Clone)]
pub struct AppContext {
    llm_manager: Arc<LLMManager>,
    #[allow(dead_code)]
    agent_manager: Arc<AgentManager>,
    tool_manager: Arc<ToolManager>,
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

        let repository = Arc::new(SqliteLlmProviderRepository::new(pool));
        let llm_manager = Arc::new(LLMManager::new(repository));
        let agent_manager = Arc::new(AgentManager::new());
        let tool_manager = Arc::new(ToolManager::new());

        Ok(Self::new(llm_manager, agent_manager, tool_manager))
    }

    #[must_use]
    pub fn new(
        llm_manager: Arc<LLMManager>,
        agent_manager: Arc<AgentManager>,
        tool_manager: Arc<ToolManager>,
    ) -> Self {
        Self {
            llm_manager,
            agent_manager,
            tool_manager,
        }
    }

    #[must_use]
    pub fn llm_manager(&self) -> Arc<LLMManager> {
        Arc::clone(&self.llm_manager)
    }

    #[must_use]
    pub fn tool_manager(&self) -> Arc<ToolManager> {
        Arc::clone(&self.tool_manager)
    }

    #[cfg(feature = "dev")]
    pub async fn upsert_provider(&self, record: LlmProviderRecord) -> Result<(), AgentError> {
        self.llm_manager.upsert_provider(record).await
    }

    #[cfg(feature = "dev")]
    pub async fn import_providers(
        &self,
        records: Vec<LlmProviderRecord>,
    ) -> Result<(), AgentError> {
        self.llm_manager.import_providers(records).await
    }

    #[cfg(feature = "dev")]
    pub async fn get_provider_record(
        &self,
        id: &LlmProviderId,
    ) -> Result<LlmProviderRecord, AgentError> {
        self.llm_manager.get_provider_record(id).await
    }

    #[cfg(feature = "dev")]
    pub async fn get_default_provider_record(&self) -> Result<LlmProviderRecord, AgentError> {
        self.llm_manager.get_default_provider_record().await
    }

    #[cfg(feature = "dev")]
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
}
