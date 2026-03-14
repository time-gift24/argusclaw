//! AgentManager - manages global managers and creates runtime Agent instances.

use std::sync::{Arc, OnceLock};

use dashmap::DashMap;

use super::runtime::{Agent, AgentBuilder};
use crate::agents::compact::CompactorManager;
use crate::agents::thread::{ThreadConfig, ThreadEvent, ThreadId};
use crate::agents::types::{AgentId, AgentRecord, AgentRepository, AgentRuntimeId};
use crate::approval::ApprovalManager;
use crate::db::DbError;
use crate::error::AgentError;
use crate::llm::{LlmProvider, LLMManager};
use crate::tool::ToolManager;

/// AgentManager creates and manages runtime Agent instances.
///
/// This is the main entry point for creating and accessing runtime agents.
/// It loads agent templates from the repository and creates runtime instances.
pub struct AgentManager {
    /// Repository for agent templates.
    repository: Arc<dyn AgentRepository>,
    /// LLM manager for building providers.
    llm_manager: Arc<LLMManager>,
    /// Global CompactorManager (shared by all agents).
    compactor_manager: Arc<CompactorManager>,
    /// Global ApprovalManager (shared by all agents).
    approval_manager: Option<Arc<ApprovalManager>>,
    /// Global ToolManager (shared by all agents).
    tool_manager: Arc<ToolManager>,
    /// Active agents indexed by runtime ID.
    agents: DashMap<AgentRuntimeId, Agent>,
    /// Default agent for simple use cases (desktop/chat).
    default_agent: OnceLock<Agent>,
}

impl AgentManager {
    /// Create a new AgentManager.
    pub fn new(
        repository: Arc<dyn AgentRepository>,
        llm_manager: Arc<LLMManager>,
        tool_manager: Arc<ToolManager>,
        approval_manager: Option<Arc<ApprovalManager>>,
    ) -> Self {
        Self {
            repository,
            llm_manager,
            compactor_manager: Arc::new(CompactorManager::with_defaults()),
            approval_manager,
            tool_manager,
            agents: DashMap::new(),
            default_agent: OnceLock::new(),
        }
    }

    /// Initialize the default agent with a provider.
    ///
    /// This is used for simple use cases where a single agent is sufficient.
    pub fn init_default_agent(&self, provider: Arc<dyn LlmProvider>) {
        let agent = AgentBuilder::new()
            .template_id(AgentId::new("default"))
            .system_prompt(String::new())
            .provider(provider)
            .tool_manager(self.tool_manager.clone())
            .compactor_manager(self.compactor_manager.clone())
            .approval_manager(self.approval_manager.clone())
            .build();
        let _ = self.default_agent.set(agent);
    }

    /// Check if default agent needs initialization.
    ///
    /// Returns true if the default agent needs to be initialized.
    #[must_use]
    pub fn needs_default_agent(&self) -> bool {
        self.default_agent.get().is_none()
    }

    /// Get or create a thread in the default agent.
    ///
    /// Creates a new thread if one doesn't exist with the given ID.
    /// Returns the thread's event receiver for subscribing to updates.
    pub fn get_or_create_thread(
        &self,
        thread_id: ThreadId,
        config: ThreadConfig,
    ) -> Result<tokio::sync::broadcast::Receiver<ThreadEvent>, AgentError> {
        let agent = self.default_agent.get().ok_or(AgentError::DefaultAgentNotInitialized)?;

        // Create thread with specific ID (or get existing)
        agent.create_thread_with_id(thread_id, config);

        // Get the thread and return its event receiver
        if let Some(thread) = agent.get_thread_mut(&thread_id) {
            Ok(thread.subscribe())
        } else {
            Err(AgentError::ThreadCreationFailed)
        }
    }

    /// Send a message to a thread in the default agent.
    pub async fn send_message(
        &self,
        thread_id: ThreadId,
        message: String,
    ) -> Result<(), AgentError> {
        let agent = self.default_agent.get().ok_or(AgentError::DefaultAgentNotInitialized)?;

        if let Some(mut thread) = agent.get_thread_mut(&thread_id) {
            thread.send_message(message).await;
            Ok(())
        } else {
            Err(AgentError::ThreadNotFound)
        }
    }

    /// Get thread messages from the default agent.
    pub fn get_thread_messages(
        &self,
        thread_id: ThreadId,
    ) -> Result<Vec<crate::llm::ChatMessage>, AgentError> {
        let agent = self.default_agent.get().ok_or(AgentError::DefaultAgentNotInitialized)?;

        if let Some(thread) = agent.get_thread_mut(&thread_id) {
            Ok(thread.history().to_vec())
        } else {
            Err(AgentError::ThreadNotFound)
        }
    }

    /// Get the compactor manager.
    #[must_use]
    pub fn compactor_manager(&self) -> &Arc<CompactorManager> {
        &self.compactor_manager
    }

    /// Get the tool manager.
    #[must_use]
    pub fn tool_manager(&self) -> &Arc<ToolManager> {
        &self.tool_manager
    }

    /// Get the approval manager.
    #[must_use]
    pub fn approval_manager(&self) -> Option<&Arc<ApprovalManager>> {
        self.approval_manager.as_ref()
    }

    /// Create a runtime Agent instance from an AgentRecord (template).
    ///
    /// Returns the `AgentRuntimeId` for accessing the agent.
    pub async fn create_agent(&self, record: &AgentRecord) -> Result<AgentRuntimeId, AgentError> {
        use crate::db::llm::LlmProviderId;
        let provider = self
            .llm_manager
            .get_provider(&LlmProviderId::new(&record.provider_id))
            .await?;

        let agent = AgentBuilder::from_record(record, provider)
            .tool_manager(self.tool_manager.clone())
            .compactor_manager(self.compactor_manager.clone())
            .approval_manager(self.approval_manager.clone())
            .build();

        let runtime_id = agent.runtime_id();
        self.agents.insert(runtime_id, agent);
        Ok(runtime_id)
    }

    /// Get an agent by runtime ID.
    #[must_use]
    pub fn get(&self, id: AgentRuntimeId) -> Option<Agent> {
        self.agents.get(&id).map(|entry| entry.value().clone())
    }

    /// Delete an agent by runtime ID.
    pub fn delete(&self, id: AgentRuntimeId) -> bool {
        self.agents.remove(&id).is_some()
    }

    /// Get the number of active agents.
    #[must_use]
    pub fn count(&self) -> usize {
        self.agents.len()
    }

    // === Template operations (delegated to repository) ===

    /// Create or update an agent template.
    pub async fn upsert_template(&self, record: AgentRecord) -> Result<(), DbError> {
        self.repository.upsert(&record).await
    }

    /// Get an agent template by ID.
    pub async fn get_template(&self, id: &AgentId) -> Result<Option<AgentRecord>, DbError> {
        self.repository.get(id).await
    }

    /// List all agent templates.
    pub async fn list_templates(&self) -> Result<Vec<AgentRecord>, DbError> {
        Ok(self
            .repository
            .list()
            .await?
            .into_iter()
            .map(|s| AgentRecord {
                id: s.id,
                display_name: s.display_name,
                description: s.description,
                version: s.version,
                provider_id: s.provider_id,
                system_prompt: String::new(),
                tool_names: vec![],
                max_tokens: None,
                temperature: None,
            })
            .collect())
    }

    /// Delete an agent template.
    pub async fn delete_template(&self, id: &AgentId) -> Result<bool, DbError> {
        self.repository.delete(id).await
    }

    /// Access the agents map for advanced operations.
    #[must_use]
    pub fn agents(&self) -> &DashMap<AgentRuntimeId, Agent> {
        &self.agents
    }
}

impl std::fmt::Debug for AgentManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentManager")
            .field("agent_count", &self.agents.len())
            .field("compactor_manager", &self.compactor_manager)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_manager_creation() {
        let tool_manager = Arc::new(ToolManager::new());
        // Note: Full test requires mock repository and LLM manager
        assert_eq!(tool_manager.list_definitions().len(), 0);
    }
}
