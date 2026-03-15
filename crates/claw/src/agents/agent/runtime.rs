//! Agent runtime implementation.

use std::sync::Arc;

use dashmap::DashMap;
use derive_builder::Builder;
use tokio::sync::broadcast;
use uuid::Uuid;

use crate::agents::compact::{Compactor, CompactorManager};
use crate::agents::thread::{Thread, ThreadBuilder, ThreadConfig, ThreadInfo};
use crate::agents::types::{AgentId, AgentRecord};
use crate::approval::{ApprovalHook, ApprovalManager, ApprovalPolicy};
use crate::error::AgentError;
use crate::llm::{ChatMessage, LlmProvider};
use crate::protocol::{ApprovalDecision, HookEvent, HookRegistry, ThreadEvent, ThreadId};
use crate::tool::ToolManager;

/// Runtime information about an agent.
#[derive(Debug, Clone)]
pub struct AgentRuntimeInfo {
    pub id: AgentId,
    pub thread_count: usize,
    pub provider_model: String,
}

impl AgentRuntimeInfo {
    #[must_use]
    pub fn new(id: AgentId, thread_count: usize, provider_model: String) -> Self {
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
/// Threads share the same provider, tool manager, and compactor manager.
#[derive(Builder)]
#[builder(pattern = "owned", build_fn(skip))]
pub struct Agent {
    /// Agent ID (from AgentRecord).
    id: AgentId,
    /// System prompt from AgentRecord.
    system_prompt: String,
    /// LLM provider (required).
    provider: Arc<dyn LlmProvider>,
    /// Tool manager.
    #[builder(default = "Arc::new(ToolManager::new())")]
    tool_manager: Arc<ToolManager>,
    /// Compactor manager.
    #[builder(default = "Arc::new(CompactorManager::with_defaults())")]
    compactor_manager: Arc<CompactorManager>,
    /// Approval manager.
    #[builder(default)]
    approval_manager: Option<Arc<ApprovalManager>>,
    /// Hook registry shared across all threads.
    #[builder(default)]
    hooks: Option<Arc<HookRegistry>>,
    /// Tools that require approval (comma-separated names).
    #[builder(default)]
    approval_tools: Vec<String>,
    /// Auto-approve all approval requests.
    #[builder(default)]
    auto_approve: bool,
    /// Active threads, protected by async Mutex for safe concurrent access.
    /// Wrapped in Arc so clones share the same thread map.
    #[builder(default)]
    threads: Arc<DashMap<ThreadId, Arc<tokio::sync::Mutex<Thread>>>>,
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
            .id(record.id.clone())
            .system_prompt(record.system_prompt.clone())
            .provider(provider)
    }

    /// Build the Agent.
    ///
    /// # Errors
    ///
    /// Returns `AgentError` if required fields (`id`, `provider`) are not set.
    pub fn build(self) -> Result<Agent, AgentError> {
        let id = self
            .id
            .ok_or(AgentError::AgentBuildFailed { field: "id" })?;
        let provider = self
            .provider
            .ok_or(AgentError::AgentBuildFailed { field: "provider" })?;

        let approval_tools = self.approval_tools.unwrap_or_default();
        let auto_approve = self.auto_approve.unwrap_or(false);

        // Build approval infrastructure if tools are specified
        let has_approval_tools = !approval_tools.is_empty();
        let approval_manager = if has_approval_tools {
            let policy = ApprovalPolicy {
                require_approval: approval_tools.clone(),
                timeout_secs: 60,
                auto_approve: false,
                auto_approve_autonomous: auto_approve,
            };
            Some(Arc::new(ApprovalManager::new(policy)))
        } else {
            self.approval_manager.flatten()
        };

        // Build hook registry
        let hooks = if let Some(existing) = self.hooks.flatten() {
            existing
        } else {
            Arc::new(HookRegistry::new())
        };

        // Register approval hook if needed
        if let Some(manager) = &approval_manager {
            let policy = manager.policy();
            let hook = ApprovalHook::new(Arc::clone(manager), policy.clone(), "agent");
            hooks.register(HookEvent::BeforeToolCall, Arc::new(hook));
        }

        Ok(Agent {
            id,
            system_prompt: self.system_prompt.unwrap_or_default(),
            provider,
            tool_manager: self
                .tool_manager
                .unwrap_or_else(|| Arc::new(ToolManager::new())),
            compactor_manager: self
                .compactor_manager
                .unwrap_or_else(|| Arc::new(CompactorManager::with_defaults())),
            approval_manager,
            hooks: Some(hooks),
            approval_tools,
            auto_approve,
            threads: Arc::new(DashMap::new()),
        })
    }
}

impl Agent {
    /// Get the agent ID.
    #[must_use]
    pub fn id(&self) -> &AgentId {
        &self.id
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
    pub fn create_thread(&self, config: ThreadConfig) -> Result<ThreadId, AgentError> {
        let compactor: Arc<dyn Compactor> = self.compactor_manager.default_compactor().clone();

        let mut builder = ThreadBuilder::new()
            .provider(self.provider.clone())
            .tool_manager(self.tool_manager.clone())
            .compactor(compactor)
            .config(config);

        if let Some(hooks) = &self.hooks {
            builder = builder.hooks(Arc::clone(hooks));
        }

        if let Some(manager) = &self.approval_manager {
            builder = builder.approval_manager(Arc::clone(manager));
        }

        let mut thread = builder.build().map_err(|e| AgentError::ThreadBuildFailed {
            reason: e.to_string(),
        })?;

        // Add system prompt as first message
        if !self.system_prompt.is_empty() {
            thread
                .messages_mut()
                .push(ChatMessage::system(&self.system_prompt));
        }

        let id = *thread.id();
        self.threads
            .insert(id, Arc::new(tokio::sync::Mutex::new(thread)));
        Ok(id)
    }

    /// Check if a thread exists.
    #[must_use]
    pub fn has_thread(&self, id: &ThreadId) -> bool {
        self.threads.contains_key(id)
    }

    /// Send a message to a thread and execute a Turn.
    pub async fn send_message(
        &self,
        thread_id: &ThreadId,
        message: String,
    ) -> Result<(), AgentError> {
        let thread_arc = self
            .threads
            .get(thread_id)
            .map(|entry| entry.value().clone())
            .ok_or(AgentError::ThreadNotFound { id: *thread_id })?;
        let mut thread = thread_arc.lock().await;
        thread.send_message(message).await;
        Ok(())
    }

    /// Subscribe to thread events.
    pub async fn subscribe(
        &self,
        thread_id: &ThreadId,
    ) -> Option<broadcast::Receiver<ThreadEvent>> {
        let thread_arc = self.threads.get(thread_id)?.value().clone();
        Some(thread_arc.lock().await.subscribe())
    }

    /// Resolve an approval request.
    pub fn resolve_approval(
        &self,
        request_id: Uuid,
        decision: ApprovalDecision,
        resolved_by: Option<String>,
    ) -> Result<(), AgentError> {
        let manager = self
            .approval_manager
            .as_ref()
            .ok_or(AgentError::ApprovalNotConfigured)?;
        manager
            .resolve(request_id, decision, resolved_by)
            .map_err(|e| AgentError::ApprovalFailed {
                reason: e.to_string(),
            })?;
        Ok(())
    }

    /// List all threads in this agent.
    #[must_use]
    pub fn list_threads(&self) -> Vec<ThreadInfo> {
        self.threads
            .iter()
            .filter_map(|entry| entry.value().try_lock().ok().map(|t| t.info()))
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
            self.provider.model_name().to_string(),
        )
    }

    /// Get the number of threads.
    #[must_use]
    pub fn thread_count(&self) -> usize {
        self.threads.len()
    }

    /// Whether auto-approve is enabled.
    #[must_use]
    pub fn auto_approve(&self) -> bool {
        self.auto_approve
    }
}

impl Clone for Agent {
    fn clone(&self) -> Self {
        Self {
            id: self.id.clone(),
            system_prompt: self.system_prompt.clone(),
            provider: self.provider.clone(),
            tool_manager: self.tool_manager.clone(),
            compactor_manager: self.compactor_manager.clone(),
            approval_manager: self.approval_manager.clone(),
            hooks: self.hooks.clone(),
            approval_tools: self.approval_tools.clone(),
            auto_approve: self.auto_approve,
            threads: self.threads.clone(),
        }
    }
}

impl std::fmt::Debug for Agent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Agent")
            .field("id", &self.id)
            .field("thread_count", &self.threads.len())
            .field("provider", &self.provider.model_name())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_agent_runtime_info() {
        let info = AgentRuntimeInfo::new(AgentId::new("test"), 5, "gpt-4".to_string());
        assert_eq!(info.thread_count, 5);
    }
}
