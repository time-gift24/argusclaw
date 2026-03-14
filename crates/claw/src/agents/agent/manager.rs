//! AgentManager - manages global managers and creates runtime Agent instances.

use std::sync::Arc;
use std::sync::OnceLock;

use dashmap::DashMap;
use tokio::sync::broadcast;

use super::runtime::{Agent, AgentBuilder};
use crate::agents::compact::CompactorManager;
use crate::agents::thread::{ThreadConfig, ThreadEvent, ThreadId};
use crate::agents::types::{AgentId, AgentRecord, AgentRepository, AgentRuntimeId};
use crate::approval::ApprovalManager;
use crate::db::DbError;
use crate::db::llm::LlmProviderId;
use crate::error::AgentError;
use crate::llm::{ChatMessage, LLMManager, LlmProvider};
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
    /// ArgusAgent's RuntimeId (cached after initialization).
    argus_agent_id: OnceLock<AgentRuntimeId>,
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
            argus_agent_id: OnceLock::new(),
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

    /// Initialize and load the ArgusAgent.
    ///
    /// This should be called once during application startup.
    /// Returns the ArgusAgent's RuntimeId for subsequent operations.
    pub async fn init_argus_agent(&self) -> Result<AgentRuntimeId, AgentError> {
        // Return cached ID if already initialized
        if let Some(id) = self.argus_agent_id.get() {
            return Ok(*id);
        }

        // Load the ArgusAgent template from database
        let record = self
            .repository
            .get(&AgentId::new("argus"))
            .await?
            .ok_or(AgentError::ArgusAgentNotFound)?;

        // Get provider - use default if not specified
        let provider = if record.provider_id.is_empty() {
            self.llm_manager.get_default_provider().await?
        } else {
            self.llm_manager
                .get_provider(&LlmProviderId::new(&record.provider_id))
                .await?
        };

        // Build the ArgusAgent
        let agent = AgentBuilder::from_record(&record, provider)
            .tool_manager(self.tool_manager.clone())
            .compactor_manager(self.compactor_manager.clone())
            .approval_manager(self.approval_manager.clone())
            .build();

        let runtime_id = agent.runtime_id();
        self.agents.insert(runtime_id, agent);

        // Cache the ArgusAgent's runtime ID
        let _ = self.argus_agent_id.set(runtime_id);

        tracing::info!("ArgusAgent initialized with runtime_id: {}", runtime_id);
        Ok(runtime_id)
    }

    /// Get the ArgusAgent's RuntimeId (if initialized).
    #[must_use]
    pub fn argus_agent_id(&self) -> Option<AgentRuntimeId> {
        self.argus_agent_id.get().copied()
    }

    /// Create a runtime Agent instance from an AgentRecord (template).
    ///
    /// Returns the `AgentRuntimeId` for accessing the agent.
    pub async fn create_agent(&self, record: &AgentRecord) -> Result<AgentRuntimeId, AgentError> {
        // Get provider - use default if not specified
        let provider = if record.provider_id.is_empty() {
            self.llm_manager.get_default_provider().await?
        } else {
            self.llm_manager
                .get_provider(&LlmProviderId::new(&record.provider_id))
                .await?
        };

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

    // === Thread operations ===

    /// Get or create a thread for a specific agent.
    ///
    /// Returns a broadcast receiver for thread events.
    pub fn get_or_create_thread(
        &self,
        agent_runtime_id: AgentRuntimeId,
        thread_id: ThreadId,
        config: ThreadConfig,
    ) -> Result<broadcast::Receiver<ThreadEvent>, AgentError> {
        let agent = self
            .agents
            .get(&agent_runtime_id)
            .ok_or(AgentError::AgentNotFound(agent_runtime_id))?
            .value()
            .clone();

        Ok(agent.get_or_create_thread(thread_id, config))
    }

    /// Switch the LLM provider for a specific thread.
    pub fn switch_thread_provider(
        &self,
        agent_runtime_id: AgentRuntimeId,
        thread_id: ThreadId,
        provider: Arc<dyn LlmProvider>,
    ) -> Result<(), AgentError> {
        let agent = self
            .agents
            .get(&agent_runtime_id)
            .ok_or(AgentError::AgentNotFound(agent_runtime_id))?
            .value()
            .clone();

        agent.switch_thread_provider(&thread_id, provider)
    }

    /// Send a message to a thread (non-blocking).
    ///
    /// The response comes through the event stream (subscribe to receive events).
    pub async fn send_message(
        &self,
        agent_runtime_id: AgentRuntimeId,
        thread_id: ThreadId,
        message: String,
    ) -> Result<(), AgentError> {
        let agent = self
            .agents
            .get(&agent_runtime_id)
            .ok_or(AgentError::AgentNotFound(agent_runtime_id))?
            .value()
            .clone();

        agent
            .send_message_to_thread(&thread_id, message)
            .ok_or(AgentError::ThreadNotFound { id: thread_id })?;

        Ok(())
    }

    /// Get messages from a thread.
    pub fn get_thread_messages(
        &self,
        agent_runtime_id: AgentRuntimeId,
        thread_id: ThreadId,
    ) -> Result<Vec<ChatMessage>, AgentError> {
        let agent = self
            .agents
            .get(&agent_runtime_id)
            .ok_or(AgentError::AgentNotFound(agent_runtime_id))?
            .value()
            .clone();

        agent
            .get_thread_messages(&thread_id)
            .ok_or(AgentError::ThreadNotFound { id: thread_id })
    }

    /// Subscribe to events from a thread.
    pub fn subscribe(
        &self,
        agent_runtime_id: AgentRuntimeId,
        thread_id: ThreadId,
    ) -> Result<broadcast::Receiver<ThreadEvent>, AgentError> {
        let agent = self
            .agents
            .get(&agent_runtime_id)
            .ok_or(AgentError::AgentNotFound(agent_runtime_id))?
            .value()
            .clone();

        agent
            .subscribe(&thread_id)
            .ok_or(AgentError::ThreadNotFound { id: thread_id })
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
