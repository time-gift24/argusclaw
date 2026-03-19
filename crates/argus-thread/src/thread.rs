//! Thread implementation.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use derive_builder::Builder;
use tokio::sync::broadcast;

use argus_protocol::llm::{ChatMessage, LlmProvider};
use argus_protocol::tool::NamedTool;
use argus_protocol::{AgentRecord, HookHandler, HookRegistry, SessionId, ThreadEvent, ThreadId};
use argus_tool::ToolManager;
use argus_turn::{TurnBuilder, TurnOutput};

use super::compact::{CompactContext, Compactor};
use super::config::ThreadConfig;
use super::error::ThreadError;
use super::types::{ThreadInfo, ThreadState};

/// Default broadcast channel capacity.
const DEFAULT_CHANNEL_CAPACITY: usize = 256;

/// Thread - multi-turn conversation session.
///
/// A Thread manages message history and executes Turns sequentially.
/// It broadcasts events to subscribers for real-time updates.
#[derive(Builder)]
#[builder(pattern = "owned", build_fn(skip))]
pub struct Thread {
    /// Unique identifier (strongly typed).
    id: ThreadId,

    /// Agent record with configuration.
    agent_record: AgentRecord,

    /// Parent session ID.
    session_id: SessionId,

    /// Optional thread title.
    #[builder(default)]
    title: Option<String>,

    /// Creation timestamp.
    #[builder(default = "Utc::now()")]
    created_at: DateTime<Utc>,

    /// Last update timestamp.
    #[builder(default = "Utc::now()")]
    updated_at: DateTime<Utc>,

    /// Initial message history (for restoring sessions).
    #[builder(default)]
    messages: Vec<ChatMessage>,

    /// LLM provider (required, injected by Session).
    provider: Arc<dyn LlmProvider>,

    /// Tool manager.
    #[builder(default = "Arc::new(ToolManager::new())")]
    tool_manager: Arc<ToolManager>,

    /// Compactor for managing context size.
    compactor: Arc<dyn Compactor>,

    /// Hook registry for lifecycle events (optional).
    #[builder(default, setter(strip_option))]
    hooks: Option<Arc<HookRegistry>>,

    /// Thread configuration.
    #[builder(default)]
    config: ThreadConfig,

    /// Token count (internal).
    #[builder(default)]
    token_count: u32,

    /// Turn count (internal).
    #[builder(default)]
    turn_count: u32,

    /// Event broadcaster (internal).
    #[builder(default)]
    event_sender: broadcast::Sender<ThreadEvent>,
}

impl std::fmt::Debug for Thread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Thread")
            .field("id", &self.id)
            .field("session_id", &self.session_id)
            .field("agent_id", &self.agent_record.id)
            .field("title", &self.title)
            .field("messages", &self.messages.len())
            .field("token_count", &self.token_count)
            .field("turn_count", &self.turn_count)
            .field("config", &self.config)
            .finish()
    }
}

impl ThreadBuilder {
    /// Create a new ThreadBuilder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build the Thread.
    ///
    /// # Errors
    ///
    /// Returns `ThreadError` if required fields (`provider`, `compactor`, `agent_record`, `session_id`) are not set.
    pub fn build(self) -> Result<Thread, ThreadError> {
        let (event_sender, _) = broadcast::channel(DEFAULT_CHANNEL_CAPACITY);

        let agent_record = self.agent_record.ok_or(ThreadError::AgentRecordNotSet)?;
        let session_id = self.session_id.ok_or(ThreadError::SessionIdNotSet)?;

        // Initialize messages with system prompt if not empty and no existing system message
        let mut messages = self.messages.unwrap_or_default();
        let has_system_message = messages
            .first()
            .is_some_and(|m| m.role == argus_protocol::llm::Role::System);
        if !has_system_message && !agent_record.system_prompt.is_empty() {
            messages.insert(0, ChatMessage::system(&agent_record.system_prompt));
        }

        Ok(Thread {
            id: self.id.unwrap_or_default(),
            agent_record,
            session_id,
            title: self.title.flatten(),
            created_at: self.created_at.unwrap_or_else(Utc::now),
            updated_at: self.updated_at.unwrap_or_else(Utc::now),
            messages,
            provider: self.provider.ok_or(ThreadError::ProviderNotConfigured)?,
            tool_manager: self
                .tool_manager
                .unwrap_or_else(|| Arc::new(ToolManager::new())),
            compactor: self.compactor.ok_or(ThreadError::CompactorNotConfigured)?,
            hooks: self.hooks.flatten(),
            config: self.config.unwrap_or_default(),
            token_count: 0,
            turn_count: 0,
            event_sender,
        })
    }
}

impl Thread {
    /// Get the Thread ID.
    pub fn id(&self) -> ThreadId {
        self.id
    }

    /// Get the Session ID.
    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    /// Get the Agent Record.
    pub fn agent_record(&self) -> &AgentRecord {
        &self.agent_record
    }

    /// Get the thread title.
    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// Set the thread title.
    pub fn set_title(&mut self, title: String) {
        self.title = Some(title);
        self.updated_at = Utc::now();
    }

    /// Get creation timestamp.
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    /// Get last update timestamp.
    pub fn updated_at(&self) -> DateTime<Utc> {
        self.updated_at
    }

    /// Get information about this thread.
    pub fn info(&self) -> ThreadInfo {
        ThreadInfo {
            id: self.id.to_string(),
            message_count: self.messages.len(),
            token_count: self.token_count,
            turn_count: self.turn_count,
        }
    }

    /// Subscribe to Thread events.
    ///
    /// Multiple subscribers can receive events simultaneously.
    pub fn subscribe(&self) -> broadcast::Receiver<ThreadEvent> {
        self.event_sender.subscribe()
    }

    /// Get current state.
    pub fn state(&self) -> ThreadState {
        ThreadState::Idle
    }

    /// Get message history (read-only).
    pub fn history(&self) -> &[ChatMessage] {
        &self.messages
    }

    /// Get current token count.
    pub fn token_count(&self) -> u32 {
        self.token_count
    }

    /// Get turn count.
    pub fn turn_count(&self) -> u32 {
        self.turn_count
    }

    /// Get the LLM provider.
    pub fn provider(&self) -> &Arc<dyn LlmProvider> {
        &self.provider
    }

    /// Get mutable access to messages (for Compactor).
    pub fn messages_mut(&mut self) -> &mut Vec<ChatMessage> {
        &mut self.messages
    }

    /// Set the token count (for Compactor).
    pub fn set_token_count(&mut self, count: u32) {
        self.token_count = count;
    }

    /// Recalculate token count from messages.
    pub fn recalculate_token_count(&mut self) {
        self.token_count = self
            .messages
            .iter()
            .map(|m| Self::estimate_tokens(&m.content))
            .sum();
    }

    fn apply_turn_output(&mut self, output: TurnOutput) {
        self.messages = output.messages;
        self.recalculate_token_count();
        self.updated_at = Utc::now();
    }

    /// Send user message and execute Turn.
    pub async fn send_message(&mut self, user_input: String) -> Result<(), ThreadError> {
        // Compactor decides internally whether to compact
        // Clone the Arc first to avoid borrow conflicts
        let compactor = self.compactor.clone();

        // Create CompactContext for compaction
        {
            let mut context =
                CompactContext::new(&self.provider, &mut self.token_count, &mut self.messages);
            if let Err(e) = compactor.compact(&mut context).await {
                tracing::warn!("Compact failed: {}", e);
            }
        }

        self.messages.push(ChatMessage::user(user_input));
        self.recalculate_token_count();
        self.execute_turn_streaming().await
    }

    async fn execute_turn_streaming(&mut self) -> Result<(), ThreadError> {
        self.turn_count += 1;
        let turn_number = self.turn_count;
        let thread_id = self.id.to_string();

        // Thread is responsible for building: collect tools and hooks
        let tools: Vec<Arc<dyn NamedTool>> = self
            .tool_manager
            .list_ids()
            .iter()
            .filter_map(|id| self.tool_manager.get(id))
            .collect();

        let hooks: Vec<Arc<dyn HookHandler>> = self
            .hooks
            .as_ref()
            .map(|registry| registry.all_handlers())
            .unwrap_or_default();

        // Create internal stream channel
        let (stream_tx, _stream_rx) = broadcast::channel(DEFAULT_CHANNEL_CAPACITY);

        // Build Turn using TurnBuilder
        let turn = TurnBuilder::default()
            .turn_number(turn_number)
            .thread_id(thread_id.clone())
            .messages(self.messages.clone())
            .provider(self.provider.clone())
            .tools(tools)
            .hooks(hooks)
            .config(self.config.turn_config.clone())
            .agent_record(Arc::new(self.agent_record.clone()))
            .stream_tx(stream_tx)
            .thread_event_tx(self.event_sender.clone())
            .build()
            .map_err(|e| ThreadError::TurnBuildFailed(e.to_string()))?;

        // Turn is responsible for execution
        let result = turn.execute().await;

        match result {
            Ok(output) => {
                self.apply_turn_output(output);
                Ok(())
            }
            Err(error) => Err(ThreadError::TurnFailed(error)),
        }
    }

    /// Estimate token count for a string.
    fn estimate_tokens(content: &str) -> u32 {
        (content.len() / 4).max(1) as u32
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::compact::KeepRecentCompactor;
    use argus_protocol::{AgentId, ProviderId};

    fn test_agent_record() -> AgentRecord {
        AgentRecord {
            id: AgentId::new(1),
            display_name: "Test Agent".to_string(),
            description: "A test agent".to_string(),
            version: "1.0.0".to_string(),
            provider_id: Some(ProviderId::new(1)),
            system_prompt: "You are a test agent.".to_string(),
            tool_names: vec![],
            max_tokens: None,
            temperature: None,
            thinking_config: None,
        }
    }

    #[test]
    fn thread_builder_requires_provider() {
        let compactor: Arc<dyn Compactor> = Arc::new(KeepRecentCompactor::with_defaults());
        let result = ThreadBuilder::new()
            .compactor(compactor)
            .agent_record(test_agent_record())
            .session_id(SessionId::new(1))
            .build();
        assert!(matches!(result, Err(ThreadError::ProviderNotConfigured)));
    }

    #[test]
    fn thread_builder_requires_compactor() {
        let result = ThreadBuilder::new()
            .agent_record(test_agent_record())
            .session_id(SessionId::new(1))
            .build();
        assert!(result.is_err());
    }

    #[test]
    fn thread_builder_requires_agent_record() {
        let compactor: Arc<dyn Compactor> = Arc::new(KeepRecentCompactor::with_defaults());
        let result = ThreadBuilder::new()
            .compactor(compactor)
            .session_id(SessionId::new(1))
            .build();
        assert!(matches!(result, Err(ThreadError::AgentRecordNotSet)));
    }

    #[test]
    fn thread_builder_requires_session_id() {
        let compactor: Arc<dyn Compactor> = Arc::new(KeepRecentCompactor::with_defaults());
        let result = ThreadBuilder::new()
            .compactor(compactor)
            .agent_record(test_agent_record())
            .build();
        assert!(matches!(result, Err(ThreadError::SessionIdNotSet)));
    }

    #[test]
    fn estimate_tokens_reasonable() {
        assert_eq!(Thread::estimate_tokens("test"), 1);
        assert_eq!(Thread::estimate_tokens("test test"), 2);
        assert_eq!(Thread::estimate_tokens(""), 1);
    }
}
