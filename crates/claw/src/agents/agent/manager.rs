//! AgentManager - manages global managers and creates runtime Agent instances.

use std::sync::Arc;
use std::sync::atomic::{AtomicI64, Ordering};

use dashmap::DashMap;
use tokio::sync::broadcast;
use uuid::Uuid;

use super::runtime::{Agent, AgentBuilder, AgentRuntimeInfo};
use crate::agents::CompactorManager;
use crate::agents::thread::{ThreadConfig, ThreadInfo};
use crate::agents::types::{AgentId, AgentRecord, AgentRepository};
use argus_approval::ApprovalManager;
use crate::db::DbError;
use crate::error::AgentError;
use argus_protocol::{ApprovalDecision, ThreadEvent, ThreadId};
use argus_llm::ProviderManager;
use argus_tool::ToolManager;

/// Starting ID for runtime agents (to distinguish from template IDs which start from 1).
const RUNTIME_AGENT_ID_START: i64 = 1_000_000_000;

/// AgentManager creates and manages runtime Agent instances.
///
/// This is the main entry point for creating and accessing runtime agents.
/// It loads agent templates from the repository and creates runtime instances.
pub struct AgentManager {
    /// Repository for agent templates.
    repository: Arc<dyn AgentRepository>,
    /// Provider manager for building providers.
    provider_manager: Arc<ProviderManager>,
    /// Global CompactorManager (shared by all agents).
    compactor_manager: Arc<CompactorManager>,
    /// Global ApprovalManager (shared by all agents).
    approval_manager: Option<Arc<ApprovalManager>>,
    /// Global ToolManager (shared by all agents).
    tool_manager: Arc<ToolManager>,
    /// Active agents indexed by agent ID.
    agents: DashMap<AgentId, Agent>,
    /// Counter for generating unique runtime agent IDs.
    runtime_id_counter: AtomicI64,
}

impl AgentManager {
    /// Create a new AgentManager.
    pub fn new(
        repository: Arc<dyn AgentRepository>,
        provider_manager: Arc<ProviderManager>,
        tool_manager: Arc<ToolManager>,
        approval_manager: Option<Arc<ApprovalManager>>,
    ) -> Self {
        Self {
            repository,
            provider_manager,
            compactor_manager: Arc::new(CompactorManager::with_defaults()),
            approval_manager,
            tool_manager,
            agents: DashMap::new(),
            runtime_id_counter: AtomicI64::new(RUNTIME_AGENT_ID_START),
        }
    }

    /// Generate a unique runtime agent ID.
    fn next_runtime_id(&self) -> AgentId {
        AgentId::new(self.runtime_id_counter.fetch_add(1, Ordering::SeqCst))
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
    /// Returns the `AgentId` for accessing the agent.
    pub async fn create_agent(&self, record: &AgentRecord) -> Result<AgentId, AgentError> {
        let provider_id = record
            .provider_id
            .ok_or(AgentError::ProviderNotConfigured {
                agent_id: record.id,
            })?;

        let provider = self.provider_manager.get_provider(&provider_id).await?;

        // Generate a unique runtime agent ID
        let runtime_id = self.next_runtime_id();

        let agent = AgentBuilder::from_record(record, provider)
            .id(runtime_id)
            .tool_manager(self.tool_manager.clone())
            .compactor_manager(self.compactor_manager.clone())
            .approval_manager(self.approval_manager.clone())
            .build()?;

        let id = *agent.id();
        self.agents.insert(id, agent);
        Ok(id)
    }

    /// Create a runtime Agent with custom approval configuration.
    ///
    /// This allows per-agent approval settings instead of using the shared approval manager.
    pub async fn create_agent_with_approval(
        &self,
        record: &AgentRecord,
        approval_tools: Vec<String>,
        auto_approve: bool,
    ) -> Result<AgentId, AgentError> {
        let provider_id = record
            .provider_id
            .ok_or(AgentError::ProviderNotConfigured {
                agent_id: record.id,
            })?;

        let provider = self.provider_manager.get_provider(&provider_id).await?;

        // Generate a unique runtime agent ID
        let runtime_id = self.next_runtime_id();

        let agent = AgentBuilder::from_record(record, provider)
            .id(runtime_id)
            .tool_manager(self.tool_manager.clone())
            .compactor_manager(self.compactor_manager.clone())
            .approval_tools(approval_tools)
            .auto_approve(auto_approve)
            .build()?;

        let id = *agent.id();
        self.agents.insert(id, agent);
        Ok(id)
    }

    /// Get an agent by ID.
    #[must_use]
    pub fn get(&self, id: &AgentId) -> Option<Agent> {
        self.agents.get(id).map(|entry| entry.value().clone())
    }

    /// Delete an agent by ID.
    pub fn delete(&self, id: &AgentId) -> bool {
        self.agents.remove(id).is_some()
    }

    /// Get the number of active agents.
    #[must_use]
    pub fn count(&self) -> usize {
        self.agents.len()
    }

    /// List all active agents.
    #[must_use]
    pub fn list_agents(&self) -> Vec<AgentRuntimeInfo> {
        self.agents
            .iter()
            .map(|entry| entry.value().runtime_info())
            .collect()
    }

    // === Thread operations (passthrough to Agent) ===

    /// Create a new thread in an agent.
    pub fn create_thread(
        &self,
        agent_id: &AgentId,
        config: ThreadConfig,
    ) -> Result<ThreadId, AgentError> {
        let agent = self
            .agents
            .get(agent_id)
            .ok_or(AgentError::AgentNotFound { id: *agent_id })?;
        agent.value().create_thread(config)
    }

    /// List all threads in an agent.
    #[must_use]
    pub fn list_threads(&self, agent_id: &AgentId) -> Option<Vec<ThreadInfo>> {
        self.agents
            .get(agent_id)
            .map(|entry| entry.value().list_threads())
    }

    /// Delete a thread from an agent.
    pub fn delete_thread(&self, agent_id: &AgentId, thread_id: &ThreadId) -> Option<bool> {
        self.agents
            .get(agent_id)
            .map(|entry| entry.value().delete_thread(thread_id))
    }

    /// Send a message to a thread.
    pub async fn send_message(
        &self,
        agent_id: &AgentId,
        thread_id: &ThreadId,
        message: String,
    ) -> Result<(), AgentError> {
        let agent = self
            .agents
            .get(agent_id)
            .ok_or(AgentError::AgentNotFound { id: *agent_id })?;
        agent.value().send_message(thread_id, message).await
    }

    /// Subscribe to thread events.
    pub async fn subscribe(
        &self,
        agent_id: &AgentId,
        thread_id: &ThreadId,
    ) -> Option<broadcast::Receiver<ThreadEvent>> {
        let agent = self.agents.get(agent_id)?;
        agent.value().subscribe(thread_id).await
    }

    /// Resolve an approval request.
    pub fn resolve_approval(
        &self,
        agent_id: &AgentId,
        request_id: Uuid,
        decision: ApprovalDecision,
        resolved_by: Option<String>,
    ) -> Result<(), AgentError> {
        let agent = self
            .agents
            .get(agent_id)
            .ok_or(AgentError::AgentNotFound { id: *agent_id })?;
        agent
            .value()
            .resolve_approval(request_id, decision, resolved_by)
    }

    /// Get a snapshot of a thread's current state.
    pub async fn get_thread_snapshot(
        &self,
        agent_id: &AgentId,
        thread_id: &ThreadId,
    ) -> Option<crate::protocol::ThreadSnapshot> {
        let agent = self.agents.get(agent_id)?;
        agent.value().get_thread_snapshot(thread_id).await
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

    /// Find an agent template by display name.
    pub async fn find_template_by_display_name(
        &self,
        display_name: &str,
    ) -> Result<Option<AgentRecord>, DbError> {
        self.repository.find_by_display_name(display_name).await
    }

    /// List all agent templates.
    pub async fn list_templates(&self) -> Result<Vec<AgentRecord>, DbError> {
        self.repository.list().await
    }

    /// Delete an agent template.
    pub async fn delete_template(&self, id: &AgentId) -> Result<bool, DbError> {
        self.repository.delete(id).await
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
