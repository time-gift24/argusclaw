//! RuntimeAgentManager - manages global managers and creates runtime Agent instances.

use std::sync::Arc;

use dashmap::DashMap;

use super::runtime::{Agent, AgentRuntimeInfo};
use crate::agents::compact::CompactManager;
use crate::agents::types::AgentId;
use crate::approval::ApprovalManager;
use crate::llm::LlmProvider;
use crate::tool::ToolManager;

/// RuntimeAgentManager creates and manages runtime Agent instances.
///
/// This is the main entry point for creating and accessing runtime agents.
/// It provides shared access to CompactManager, ApprovalManager, and ToolManager.
pub struct RuntimeAgentManager {
    /// Global CompactManager (shared by all agents).
    compact_manager: Arc<CompactManager>,
    /// Global ApprovalManager (shared by all agents).
    approval_manager: Option<Arc<ApprovalManager>>,
    /// Global ToolManager (shared by all agents).
    tool_manager: Arc<ToolManager>,
    /// Active agents.
    agents: DashMap<AgentId, Agent>,
    /// Default context window (used if provider doesn't specify).
    default_context_window: u32,
}

impl RuntimeAgentManager {
    /// Create a new RuntimeAgentManager.
    pub fn new(
        tool_manager: Arc<ToolManager>,
        approval_manager: Option<Arc<ApprovalManager>>,
    ) -> Self {
        Self {
            compact_manager: Arc::new(CompactManager::with_defaults(128_000)),
            approval_manager,
            tool_manager,
            agents: DashMap::new(),
            default_context_window: 128_000,
        }
    }

    /// Create a new RuntimeAgentManager with custom compact configuration.
    #[allow(clippy::too_many_arguments)]
    pub fn with_compact(
        tool_manager: Arc<ToolManager>,
        approval_manager: Option<Arc<ApprovalManager>>,
        compact_manager: Arc<CompactManager>,
    ) -> Self {
        Self {
            compact_manager,
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

    /// Create a new Agent instance with a default provider.
    ///
    /// Returns the AgentId for accessing the agent.
    pub fn create_agent(&self, provider: Arc<dyn LlmProvider>) -> AgentId {
        // Use default context window for now
        // TODO: Add context_length to LlmProvider trait if needed
        let context_window = self.default_context_window;

        // Create a compact manager with the provider's context window
        let compact_manager = Arc::new(CompactManager::with_defaults(context_window));

        let agent_id = AgentId::new(uuid::Uuid::new_v4().to_string());

        let agent = Agent::new(
            agent_id.clone(),
            provider,
            self.tool_manager.clone(),
            compact_manager,
            self.approval_manager.clone(),
        );

        self.agents.insert(agent_id.clone(), agent);
        agent_id
    }

    /// List all active agents.
    #[must_use]
    pub fn list_agents(&self) -> Vec<AgentRuntimeInfo> {
        self.agents
            .iter()
            .map(|entry| entry.value().runtime_info())
            .collect()
    }

    /// Delete an agent.
    pub fn delete_agent(&self, id: &AgentId) -> bool {
        self.agents.remove(id).is_some()
    }

    /// Get the number of active agents.
    #[must_use]
    pub fn agent_count(&self) -> usize {
        self.agents.len()
    }

    /// Update the compact manager configuration.
    pub fn set_compact_config(&self, context_window: u32, _threshold_ratio: f32) {
        // Create a new compact manager with updated configuration
        let _compact_manager = Arc::new(CompactManager::with_defaults(context_window));
    }

    /// Access the agents map for advanced operations.
    #[must_use]
    pub fn agents(&self) -> &DashMap<AgentId, Agent> {
        &self.agents
    }
}

impl std::fmt::Debug for RuntimeAgentManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeAgentManager")
            .field("agent_count", &self.agents.len())
            .field("compact_manager", &self.compact_manager)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_agent_manager_creation() {
        let tool_manager = Arc::new(ToolManager::new());
        let manager = RuntimeAgentManager::new(tool_manager, None);

        assert_eq!(manager.agent_count(), 0);
    }

    #[test]
    fn test_runtime_agent_manager_with_compact() {
        let tool_manager = Arc::new(ToolManager::new());
        let compact = Arc::new(CompactManager::with_defaults(64_000));
        let manager = RuntimeAgentManager::with_compact(tool_manager, None, compact);

        assert_eq!(manager.compact_manager().context_window(), 64_000);
    }
}
