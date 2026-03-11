//! Agent and AgentHandle implementations.

use std::fmt;
use std::sync::Arc;

use dashmap::DashMap;

use crate::agents::compact::CompactManager;
use crate::agents::thread::{Thread, ThreadConfig, ThreadId, ThreadInfo};
use crate::approval::ApprovalManager;
use crate::llm::LlmProvider;
use crate::tool::ToolManager;

/// Runtime information about an agent.
#[derive(Debug, Clone)]
pub struct AgentRuntimeInfo {
    pub id: crate::agents::types::AgentId,
    pub thread_count: usize,
    pub provider_model: String,
}

impl AgentRuntimeInfo {
    #[must_use]
    pub fn new(id: crate::agents::types::AgentId, thread_count: usize, provider_model: String) -> Self {
        Self {
            id,
            thread_count,
            provider_model,
        }
    }
}

/// An Agent manages multiple conversation threads with shared configuration.
///
/// Each agent has a default LLM provider and manages multiple threads.
/// Threads share the same provider, tool manager, and compact manager.
pub struct Agent {
    /// Unique agent ID.
    id: crate::agents::types::AgentId,
    /// Default provider for new threads.
    default_provider: Arc<dyn LlmProvider>,
    /// Tool manager (shared).
    tool_manager: Arc<ToolManager>,
    /// Compact manager (shared).
    compact_manager: Arc<CompactManager>,
    /// Approval manager (optional).
    approval_manager: Option<Arc<ApprovalManager>>,
    /// Active threads managed by this agent.
    threads: DashMap<ThreadId, Thread>,
}

impl Agent {
    /// Create a new Agent.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn new(
        id: crate::agents::types::AgentId,
        default_provider: Arc<dyn LlmProvider>,
        tool_manager: Arc<ToolManager>,
        compact_manager: Arc<CompactManager>,
        approval_manager: Option<Arc<ApprovalManager>>,
    ) -> Self {
        Self {
            id,
            default_provider,
            tool_manager,
            compact_manager,
            approval_manager,
            threads: DashMap::new(),
        }
    }

    /// Get the agent ID.
    #[must_use]
    pub fn id(&self) -> &crate::agents::types::AgentId {
        &self.id
    }

    /// Get the default provider.
    #[must_use]
    pub fn provider(&self) -> &Arc<dyn LlmProvider> {
        &self.default_provider
    }

    /// Get the tool manager.
    #[must_use]
    pub fn tool_manager(&self) -> &Arc<ToolManager> {
        &self.tool_manager
    }

    /// Get the compact manager.
    #[must_use]
    pub fn compact_manager(&self) -> &Arc<CompactManager> {
        &self.compact_manager
    }

    /// Get the approval manager (if configured).
    #[must_use]
    pub fn approval_manager(&self) -> Option<&Arc<ApprovalManager>> {
        self.approval_manager.as_ref()
    }

    /// Create a new thread in this agent.
    pub fn create_thread(&self, config: ThreadConfig) -> ThreadId {
        let thread = Thread::new(
            self.default_provider.clone(),
            self.tool_manager.clone(),
            self.compact_manager.clone(),
            self.approval_manager.clone(),
            config,
        );
        let id = thread.id().clone();
        self.threads.insert(id.clone(), thread);
        id
    }

    /// Get a thread by ID.
    #[must_use]
    pub fn get_thread(&self, id: &ThreadId) -> Option<AgentHandle> {
        self.threads
            .get(id)
            .map(|entry| AgentHandle {
                id: id.clone(),
                agent_id: self.id.clone(),
            })
    }

    /// Get mutable reference to a thread by ID.
    /// Note: This holds a write lock on the thread map.
    #[must_use]
    pub fn get_thread_mut(&self, id: &ThreadId) -> Option<dashmap::mapref::one::RefMut<'_, ThreadId, Thread>> {
        self.threads.get_mut(id)
    }

    /// Send a message to a thread.
    ///
    /// This is a convenience method that creates a thread if it doesn't exist.
    pub fn send_message(&self, thread_id: &ThreadId, message: String) -> Option<AgentHandle> {
        // Check if thread exists, if not return None
        self.threads
            .get(thread_id)
            .map(|entry| AgentHandle {
                id: thread_id.clone(),
                agent_id: self.id.clone(),
            })
    }

    /// List all threads in this agent.
    #[must_use]
    pub fn list_threads(&self) -> Vec<ThreadInfo> {
        self.threads
            .iter()
            .map(|entry| entry.value().info())
            .collect()
    }

    /// Delete a thread.
    pub fn delete_thread(&self, id: &ThreadId) -> bool {
        self.threads.remove(id).is_some()
    }

    /// Get runtime info about this agent.
    #[must_use]
    pub fn runtime_info(&self) -> AgentRuntimeInfo {
        AgentRuntimeInfo::new(
            self.id.clone(),
            self.threads.len(),
            self.default_provider.model_name().to_string(),
        )
    }

    /// Get the number of threads.
    #[must_use]
    pub fn thread_count(&self) -> usize {
        self.threads.len()
    }
}

impl std::fmt::Debug for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Agent")
            .field("id", &self.id)
            .field("thread_count", &self.threads.len())
            .field("provider", &self.default_provider.model_name())
            .finish()
    }
}

/// A handle for accessing a thread through an agent.
///
/// This is a lightweight handle that allows access to thread operations
/// without needing a reference to the full Agent.
#[derive(Clone)]
pub struct AgentHandle {
    /// Thread ID.
    id: ThreadId,
    /// Agent ID (for reference).
    agent_id: crate::agents::types::AgentId,
}

impl AgentHandle {
    /// Create a new AgentHandle.
    #[must_use]
    pub fn new(id: ThreadId, agent_id: crate::agents::types::AgentId) -> Self {
        Self { id, agent_id }
    }

    /// Get the thread ID.
    #[must_use]
    pub fn id(&self) -> &ThreadId {
        &self.id
    }

    /// Get the agent ID.
    #[must_use]
    pub fn agent_id(&self) -> &crate::agents::types::AgentId {
        &self.agent_id
    }
}

impl std::fmt::Debug for AgentHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("AgentHandle")
            .field("id", &self.id)
            .field("agent_id", &self.agent_id)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_info() {
        // Just test the basic struct construction
        let info = AgentRuntimeInfo::new(crate::agents::types::AgentId::new("test"), 5, "gpt-4".to_string());
        assert_eq!(info.thread_count, 5);
    }

    #[test]
    fn test_thread_handle() {
        let handle = AgentHandle::new(
            ThreadId::new("thread-1"),
            crate::agents::types::AgentId::new("agent-1"),
        );
        assert_eq!(handle.id().to_string(), "thread-1");
        assert_eq!(handle.agent_id().to_string(), "agent-1");
    }
}
