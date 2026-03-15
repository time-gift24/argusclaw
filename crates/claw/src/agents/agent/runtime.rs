//! Agent and AgentHandle implementations.

use std::sync::Arc;

use dashmap::DashMap;
use derive_builder::Builder;

use crate::agents::compact::{Compactor, CompactorManager};
use crate::agents::thread::{
    Thread, ThreadBuilder, ThreadConfig, ThreadEvent, ThreadId, ThreadInfo, TurnStreamHandle,
};
use crate::agents::turn::{TurnStreamEvent, execute_turn_streaming};
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

    /// Switch the LLM provider for a specific thread.
    ///
    /// This allows changing the provider at the thread level,
    /// enabling different threads to use different providers.
    pub fn switch_thread_provider(
        &self,
        thread_id: &ThreadId,
        provider: Arc<dyn LlmProvider>,
    ) -> Result<(), crate::error::AgentError> {
        if let Some(mut thread) = self.threads.get_mut(thread_id) {
            thread.switch_provider(provider);
            Ok(())
        } else {
            Err(crate::error::AgentError::ThreadNotFound { id: *thread_id })
        }
    }

    /// Get or create a thread with a specific ID and configuration.
    ///
    /// Returns a receiver for thread events.
    pub fn get_or_create_thread(
        &self,
        thread_id: ThreadId,
        config: ThreadConfig,
    ) -> tokio::sync::broadcast::Receiver<ThreadEvent> {
        // Check if thread exists
        if let Some(thread) = self.threads.get(&thread_id) {
            return thread.subscribe();
        }

        // Create new thread with the specified ID using ThreadBuilder
        let compactor: Arc<dyn Compactor> = self.compactor_manager.default_compactor().clone();

        let mut builder = ThreadBuilder::new()
            .id(thread_id)
            .provider(self.provider.clone())
            .tool_manager(self.tool_manager.clone())
            .compactor(compactor)
            .config(config);

        // Conditionally set approval_manager if present
        if let Some(am) = self.approval_manager.clone() {
            builder = builder.approval_manager(am);
        }

        let thread = builder.build();

        let event_rx = thread.subscribe();
        self.threads.insert(thread_id, thread);
        event_rx
    }

    /// Subscribe to events from a specific thread.
    pub fn subscribe(
        &self,
        thread_id: &ThreadId,
    ) -> Option<tokio::sync::broadcast::Receiver<ThreadEvent>> {
        self.threads.get(thread_id).map(|t| t.subscribe())
    }

    /// Get messages from a specific thread.
    pub fn get_thread_messages(
        &self,
        thread_id: &ThreadId,
    ) -> Option<Vec<crate::llm::ChatMessage>> {
        self.threads.get(thread_id).map(|t| t.history().to_vec())
    }

    pub fn apply_turn_output(
        &self,
        thread_id: &ThreadId,
        output: crate::agents::turn::TurnOutput,
    ) -> bool {
        if let Some(mut thread) = self.threads.get_mut(thread_id) {
            thread.apply_turn_output(output);
            true
        } else {
            false
        }
    }

    /// Start a turn on a live thread and keep thread state synchronized before final events.
    pub async fn send_message_to_thread(
        self: &Arc<Self>,
        thread_id: &ThreadId,
        message: String,
    ) -> Option<TurnStreamHandle> {
        let pending = {
            let mut thread = self.threads.get_mut(thread_id)?;
            thread.prepare_turn(message).await
        };
        let crate::agents::thread::PendingTurn {
            handle,
            thread_id,
            turn_number,
            turn_input,
            config,
            mut stream_rx,
            llm_event_tx,
            result_tx,
            event_sender,
        } = pending;

        let agent = Arc::clone(self);
        tokio::spawn(async move {
            let forwarder_event_sender = event_sender.clone();

            let forwarder = tokio::spawn(async move {
                while let Ok(event) = stream_rx.recv().await {
                    if let TurnStreamEvent::LlmEvent(llm_event) = event {
                        let _ = forwarder_event_sender.send(ThreadEvent::Processing {
                            thread_id,
                            turn_number,
                            event: llm_event.clone(),
                        });
                        let _ = llm_event_tx.send(llm_event);
                    }
                }
            });

            let result = execute_turn_streaming(turn_input, config).await;
            let _ = forwarder.await;

            match &result {
                Ok(output) => {
                    if !agent.apply_turn_output(&thread_id, output.clone()) {
                        tracing::warn!(
                            "Thread {} disappeared before turn result could be applied",
                            thread_id
                        );
                    }

                    let _ = event_sender.send(ThreadEvent::TurnCompleted {
                        thread_id,
                        turn_number,
                        token_usage: output.token_usage.clone(),
                    });
                }
                Err(error) => {
                    let _ = event_sender.send(ThreadEvent::TurnFailed {
                        thread_id,
                        turn_number,
                        error: error.to_string(),
                    });
                }
            }

            let _ = event_sender.send(ThreadEvent::Idle { thread_id });
            let _ = result_tx.send(result);
        });

        Some(handle)
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
