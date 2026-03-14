//! Agent and AgentHandle implementations.

use std::sync::Arc;

use dashmap::DashMap;
use derive_builder::Builder;

use crate::agents::compact::{Compactor, CompactorManager};
use crate::agents::thread::{Thread, ThreadConfig, ThreadId, ThreadInfo};
use crate::agents::types::{AgentId, AgentRecord, AgentRuntimeId};
use crate::approval::ApprovalManager;
use crate::llm::LlmProvider;
use crate::tool::ToolManager;

/// Runtime information about an agent.
#[derive(Debug, Clone)]
pub struct AgentRuntimeInfo {
    pub template_id: AgentId,
    pub runtime_id: AgentRuntimeId,
    pub thread_count: usize,
    pub provider_model: String,
}

impl AgentRuntimeInfo {
    #[must_use]
    pub fn new(
        template_id: AgentId,
        runtime_id: AgentRuntimeId,
        thread_count: usize,
        provider_model: String,
    ) -> Self {
        Self {
            template_id,
            runtime_id,
            thread_count,
            provider_model,
        }
    }
}

/// An Agent manages multiple conversation threads with shared configuration.
///
/// Each agent has a default LLM provider and manages multiple threads.
/// Threads share the same provider, tool manager, and compactor manager.
#[derive(Builder)]
#[builder(pattern = "owned", build_fn(skip))]
pub struct Agent {
    /// Template ID from AgentRecord.
    pub template_id: AgentId,
    /// Unique runtime instance ID.
    #[builder(default = AgentRuntimeId::new())]
    pub runtime_id: AgentRuntimeId,
    /// System prompt from AgentRecord.
    pub system_prompt: String,
    /// LLM provider (required).
    pub provider: Arc<dyn LlmProvider>,
    /// Tool manager.
    #[builder(default = "Arc::new(ToolManager::new())")]
    pub tool_manager: Arc<ToolManager>,
    /// Compactor manager.
    #[builder(default = "Arc::new(CompactorManager::with_defaults())")]
    pub compactor_manager: Arc<CompactorManager>,
    /// Approval manager.
    #[builder(default)]
    pub approval_manager: Option<Arc<ApprovalManager>>,
    /// Active threads.
    #[builder(default)]
    threads: DashMap<ThreadId, Thread>,
}

impl AgentBuilder {
    /// Create a new AgentBuilder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an AgentBuilder from an AgentRecord and provider.
    #[must_use]
    pub fn from_record(record: &AgentRecord, provider: Arc<dyn LlmProvider>) -> Self {
        Self::default()
            .template_id(record.id.clone())
            .system_prompt(record.system_prompt.clone())
            .provider(provider)
    }

    /// Build the Agent.
    #[must_use]
    pub fn build(self) -> Agent {
        Agent {
            template_id: self.template_id.expect("template_id is required"),
            runtime_id: self.runtime_id.unwrap_or_default(),
            system_prompt: self.system_prompt.unwrap_or_default(),
            provider: self.provider.expect("provider is required"),
            tool_manager: self
                .tool_manager
                .unwrap_or_else(|| Arc::new(ToolManager::new())),
            compactor_manager: self
                .compactor_manager
                .unwrap_or_else(|| Arc::new(CompactorManager::with_defaults())),
            approval_manager: self.approval_manager.flatten(),
            threads: DashMap::new(),
        }
    }
}

impl Agent {
    /// Get the template ID.
    #[must_use]
    pub fn template_id(&self) -> &AgentId {
        &self.template_id
    }

    /// Get the runtime ID.
    #[must_use]
    pub fn runtime_id(&self) -> AgentRuntimeId {
        self.runtime_id
    }

    /// Get the provider.
    #[must_use]
    pub fn provider(&self) -> &Arc<dyn LlmProvider> {
        &self.provider
    }

    /// Get the tool manager.
    #[must_use]
    pub fn tool_manager(&self) -> &Arc<ToolManager> {
        &self.tool_manager
    }

    /// Get the compactor manager.
    #[must_use]
    pub fn compactor_manager(&self) -> &Arc<CompactorManager> {
        &self.compactor_manager
    }

    /// Get the approval manager (if configured).
    #[must_use]
    pub fn approval_manager(&self) -> Option<&Arc<ApprovalManager>> {
        self.approval_manager.as_ref()
    }

    /// Create a new thread in this agent.
    pub fn create_thread(&self, config: ThreadConfig) -> ThreadId {
        let compactor: Arc<dyn Compactor> = self.compactor_manager.default_compactor().clone();

        let thread = Thread::new(
            self.provider.clone(),
            self.tool_manager.clone(),
            compactor,
            self.approval_manager.clone(),
            config,
        );
        let id = *thread.id();
        self.threads.insert(id, thread);
        id
    }

    /// Create a new thread with a specific ID, or get existing one.
    ///
    /// Returns true if a new thread was created, false if it already existed.
    pub fn create_thread_with_id(&self, thread_id: ThreadId, config: ThreadConfig) -> bool {
        // Check if already exists
        if self.threads.contains_key(&thread_id) {
            return false;
        }

        let compactor: Arc<dyn Compactor> = self.compactor_manager.default_compactor().clone();

        let mut thread = Thread::new(
            self.provider.clone(),
            self.tool_manager.clone(),
            compactor,
            self.approval_manager.clone(),
            config,
        );
        // Override the generated ID with the requested one
        thread.id = thread_id;
        self.threads.insert(thread_id, thread);
        true
    }

    /// Get a thread by ID.
    #[must_use]
    pub fn get_thread(&self, id: &ThreadId) -> Option<AgentHandle> {
        self.threads.get(id).map(|_entry| AgentHandle {
            id: *id,
            runtime_id: self.runtime_id,
        })
    }

    /// Get mutable reference to a thread by ID.
    #[must_use]
    pub fn get_thread_mut(
        &self,
        id: &ThreadId,
    ) -> Option<dashmap::mapref::one::RefMut<'_, ThreadId, Thread>> {
        self.threads.get_mut(id)
    }

    /// Send a message to a thread.
    pub fn send_message(&self, thread_id: &ThreadId, _message: String) -> Option<AgentHandle> {
        self.threads.get(thread_id).map(|_entry| AgentHandle {
            id: *thread_id,
            runtime_id: self.runtime_id,
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
            self.template_id.clone(),
            self.runtime_id,
            self.threads.len(),
            self.provider.model_name().to_string(),
        )
    }

    /// Get the number of threads.
    #[must_use]
    pub fn thread_count(&self) -> usize {
        self.threads.len()
    }
}

impl Clone for Agent {
    fn clone(&self) -> Self {
        Self {
            template_id: self.template_id.clone(),
            runtime_id: self.runtime_id,
            system_prompt: self.system_prompt.clone(),
            provider: self.provider.clone(),
            tool_manager: self.tool_manager.clone(),
            compactor_manager: self.compactor_manager.clone(),
            approval_manager: self.approval_manager.clone(),
            threads: DashMap::new(),
        }
    }
}

impl std::fmt::Debug for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Agent")
            .field("template_id", &self.template_id)
            .field("runtime_id", &self.runtime_id)
            .field("thread_count", &self.threads.len())
            .field("provider", &self.provider.model_name())
            .finish()
    }
}

/// A handle for accessing a thread through an agent.
#[derive(Clone)]
pub struct AgentHandle {
    /// Thread ID.
    id: ThreadId,
    /// Agent runtime ID.
    runtime_id: AgentRuntimeId,
}

impl AgentHandle {
    /// Create a new AgentHandle.
    #[must_use]
    pub fn new(id: ThreadId, runtime_id: AgentRuntimeId) -> Self {
        Self { id, runtime_id }
    }

    /// Get the thread ID.
    #[must_use]
    pub fn id(&self) -> &ThreadId {
        &self.id
    }

    /// Get the agent runtime ID.
    #[must_use]
    pub fn runtime_id(&self) -> AgentRuntimeId {
        self.runtime_id
    }
}

impl std::fmt::Debug for AgentHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AgentHandle")
            .field("id", &self.id)
            .field("runtime_id", &self.runtime_id)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_runtime_info() {
        let info = AgentRuntimeInfo::new(
            AgentId::new("test"),
            AgentRuntimeId::new(),
            5,
            "gpt-4".to_string(),
        );
        assert_eq!(info.thread_count, 5);
    }

    #[test]
    fn test_agent_handle() {
        let thread_id = ThreadId::new();
        let runtime_id = AgentRuntimeId::new();
        let handle = AgentHandle::new(thread_id, runtime_id);
        assert_eq!(*handle.id(), thread_id);
        assert_eq!(handle.runtime_id(), runtime_id);
    }
}
