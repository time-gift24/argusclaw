//! AgentManager - manages global managers and creates runtime Agent instances.

use std::sync::Arc;

use dashmap::DashMap;

use super::runtime::{Agent, AgentBuilder};
use crate::agents::compact::CompactManager;
use crate::agents::types::{AgentId, AgentRecord, AgentRepository, AgentRuntimeId};
use crate::approval::ApprovalManager;
use crate::db::DbError;
use crate::error::AgentError;
use crate::llm::LLMManager;
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
    /// Global CompactManager (shared by all agents).
    compact_manager: Arc<CompactManager>,
    /// Global ApprovalManager (shared by all agents).
    approval_manager: Option<Arc<ApprovalManager>>,
    /// Global ToolManager (shared by all agents).
    tool_manager: Arc<ToolManager>,
    /// Active agents indexed by runtime ID.
    agents: DashMap<AgentRuntimeId, Agent>,
    /// Default context window (used if provider doesn't specify).
    default_context_window: u32,
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
            compact_manager: Arc::new(CompactManager::with_defaults(128_000)),
            approval_manager,
            tool_manager,
            agents: DashMap::new(),
            default_context_window: 128_000,
        }
    }

    /// Get the compact manager.
    #[must_use]
    pub fn compact_manager(&self) -> &Arc<CompactManager> {
        &self.compact_manager
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

        // Use default context window for now
        let context_window = self.default_context_window;
        let compact_manager = Arc::new(CompactManager::with_defaults(context_window));

        let agent = AgentBuilder::from_record(record, provider)
            .tool_manager(self.tool_manager.clone())
            .compact_manager(Some(compact_manager))
            .approval_manager(self.approval_manager.clone())
            .build();

        let runtime_id = agent.runtime_id;
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
            .field("compact_manager", &self.compact_manager)
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
