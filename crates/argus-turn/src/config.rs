//! Turn configuration, input, and output types.
//!
//! This module defines the configuration options and data structures used
//! for turn-based LLM conversation execution with tool support.

use std::sync::Arc;

use derive_builder::Builder;
use tokio::sync::broadcast;

use argus_protocol::llm::{ChatMessage, LlmProvider, LlmStreamEvent};
use argus_protocol::{AgentRecord, HookRegistry, SafetyConfig, ThreadEvent};
use argus_tool::ToolManager;

use super::TraceConfig;

/// Callback invoked after a turn completes, with session_id and turn_number.
pub type OnTurnComplete = Arc<dyn Fn(argus_protocol::SessionId, u32) + Send + Sync>;

/// Turn execution configuration.
///
/// Controls the behavior of a turn execution, including limits on tool calls,
/// timeouts, and iteration counts.
#[derive(Clone, Builder)]
pub struct TurnConfig {
    /// Maximum tool calls per LLM response.
    ///
    /// When set, limits the number of tool calls executed from a single LLM response.
    /// If the LLM requests more tools than this limit, only the first N tools are executed,
    /// forcing the LLM to proceed step-by-step in subsequent iterations.
    /// Set to `None` or `Some(0)` to allow unlimited parallel tool calls.
    #[builder(default = Some(10))]
    pub max_tool_calls: Option<u32>,
    /// Maximum duration for a single tool execution (seconds).
    #[builder(default = Some(120))]
    pub tool_timeout_secs: Option<u64>,
    /// Maximum number of loop iterations (LLM -> Tool -> LLM cycles).
    #[builder(default = Some(50))]
    pub max_iterations: Option<u32>,
    /// Safety configuration for tool output sanitization.
    #[builder(default = "SafetyConfig::new()")]
    pub safety_config: SafetyConfig,
    /// Trace configuration for turn execution logging.
    #[builder(default, setter(strip_option))]
    pub trace_config: Option<TraceConfig>,
    /// Callback invoked after a turn completes.
    #[builder(default, setter(strip_option))]
    pub on_turn_complete: Option<OnTurnComplete>,
}

impl std::fmt::Debug for TurnConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TurnConfig")
            .field("max_tool_calls", &self.max_tool_calls)
            .field("tool_timeout_secs", &self.tool_timeout_secs)
            .field("max_iterations", &self.max_iterations)
            .field("safety_config", &self.safety_config)
            .field("trace_config", &self.trace_config)
            .field("on_turn_complete", &self.on_turn_complete.is_some())
            .finish()
    }
}

impl TurnConfig {
    /// Create a new TurnConfig with default values.
    pub fn new() -> Self {
        Self {
            max_tool_calls: Some(10),
            tool_timeout_secs: Some(120),
            max_iterations: Some(50),
            safety_config: SafetyConfig::new(),
            trace_config: None,
            on_turn_complete: None,
        }
    }
}

impl Default for TurnConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Streaming execution events emitted during turn execution.
///
/// These events are sent through the `stream_sender` when using
/// `execute_turn_streaming` for real-time UI updates.
#[derive(Debug, Clone)]
pub enum TurnStreamEvent {
    /// LLM stream event (content delta, reasoning, tool call delta).
    LlmEvent(LlmStreamEvent),
    /// Tool execution started.
    ToolStarted {
        /// Tool call ID.
        tool_call_id: String,
        /// Tool name.
        tool_name: String,
        /// Tool arguments.
        arguments: serde_json::Value,
    },
    /// Tool execution completed.
    ToolCompleted {
        /// Tool call ID.
        tool_call_id: String,
        /// Tool name.
        tool_name: String,
        /// Tool result (Ok for success, Err for failure).
        result: Result<serde_json::Value, String>,
    },
}

/// Input for a Turn execution.
///
/// Contains all the data needed to execute a single turn in a conversation,
/// including message history, system prompt, LLM provider, and tools.
///
/// Note: `system_prompt` is not automatically added to messages. The caller
/// should include system instructions in the `messages` field if needed.
#[derive(Builder)]
#[builder(pattern = "owned", build_fn(skip))]
pub struct TurnInput {
    /// Historical messages for the conversation.
    #[builder(default)]
    pub messages: Vec<ChatMessage>,
    /// System prompt for this turn (caller should include in messages if needed).
    #[builder(default, setter(into))]
    pub system_prompt: String,
    /// LLM provider instance.
    pub provider: Arc<dyn LlmProvider>,
    /// Tool manager for registry.
    #[builder(default = "Arc::new(ToolManager::new())")]
    pub tool_manager: Arc<ToolManager>,
    /// Tool IDs to use (resolved via ToolManager).
    #[builder(default)]
    pub tool_ids: Vec<String>,
    /// Optional hook registry for lifecycle events.
    #[builder(default, setter(strip_option))]
    pub hooks: Option<Arc<HookRegistry>>,
    /// Thread event sender for broadcasting approval events.
    #[builder(default, setter(strip_option))]
    pub thread_event_sender: Option<broadcast::Sender<ThreadEvent>>,
    /// Thread ID for event context.
    #[builder(default, setter(strip_option))]
    pub thread_id: Option<String>,
    /// Stream event sender for real-time updates during streaming execution.
    #[builder(default, setter(strip_option))]
    pub stream_sender: Option<broadcast::Sender<TurnStreamEvent>>,
    /// Agent record for LLM configuration (max_tokens, temperature, thinking_config).
    #[builder(default)]
    pub agent_record: Arc<AgentRecord>,
}

impl std::fmt::Debug for TurnInput {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TurnInput")
            .field("messages", &self.messages.len())
            .field("system_prompt", &self.system_prompt)
            .field("provider", &self.provider.model_name())
            .field("tool_manager", &"ToolManager")
            .field("tool_ids", &self.tool_ids)
            .field("hooks", &self.hooks.is_some())
            .field("thread_event_sender", &self.thread_event_sender.is_some())
            .field("thread_id", &self.thread_id)
            .field("stream_sender", &self.stream_sender.is_some())
            .finish()
    }
}

impl TurnInputBuilder {
    /// Create a new builder.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Build the TurnInput.
    ///
    /// # Errors
    ///
    /// Returns `TurnError` if the required `provider` field is not set.
    pub fn build(self) -> Result<TurnInput, super::TurnError> {
        Ok(TurnInput {
            messages: self.messages.unwrap_or_default(),
            system_prompt: self.system_prompt.unwrap_or_default(),
            provider: self
                .provider
                .ok_or(super::TurnError::ProviderNotConfigured)?,
            tool_manager: self
                .tool_manager
                .unwrap_or_else(|| Arc::new(ToolManager::new())),
            tool_ids: self.tool_ids.unwrap_or_default(),
            hooks: self.hooks.flatten(),
            thread_event_sender: self.thread_event_sender.flatten(),
            thread_id: self.thread_id.flatten(),
            stream_sender: self.stream_sender.flatten(),
            agent_record: self
                .agent_record
                .unwrap_or_else(|| Arc::new(AgentRecord::default())),
        })
    }
}

/// Output from a Turn execution.
///
/// Contains the results of executing a turn, including the updated
/// message history and token usage statistics.
#[derive(Debug, Clone, Builder)]
pub struct TurnOutput {
    /// Updated message history (includes assistant response + tool results).
    pub messages: Vec<ChatMessage>,
    /// Token usage statistics.
    #[builder(default)]
    pub token_usage: argus_protocol::TokenUsage,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_turn_config_defaults() {
        let config = TurnConfig::new();
        assert_eq!(config.max_tool_calls, Some(10));
        assert_eq!(config.tool_timeout_secs, Some(120));
        assert_eq!(config.max_iterations, Some(50));
        assert!(config.trace_config.is_none());
    }

    #[test]
    fn test_turn_config_default_trait() {
        let config = TurnConfig::default();
        assert_eq!(config.max_tool_calls, Some(10));
        assert_eq!(config.tool_timeout_secs, Some(120));
        assert_eq!(config.max_iterations, Some(50));
        assert!(config.trace_config.is_none());
    }

    #[test]
    fn test_turn_config_builder() {
        let config = TurnConfigBuilder::default()
            .max_tool_calls(Some(5))
            .tool_timeout_secs(Some(60))
            .max_iterations(Some(20))
            .build()
            .unwrap();
        assert_eq!(config.max_tool_calls, Some(5));
        assert_eq!(config.tool_timeout_secs, Some(60));
        assert_eq!(config.max_iterations, Some(20));
    }

    #[test]
    fn test_turn_config_builder_partial() {
        let config = TurnConfigBuilder::default()
            .max_tool_calls(Some(3))
            .build()
            .unwrap();
        assert_eq!(config.max_tool_calls, Some(3));
        assert_eq!(config.tool_timeout_secs, Some(120)); // default
        assert_eq!(config.max_iterations, Some(50)); // default
    }

    #[test]
    fn test_turn_output_builder() {
        let output = TurnOutputBuilder::default()
            .messages(Vec::new())
            .build()
            .unwrap();
        assert!(output.messages.is_empty());
        assert_eq!(output.token_usage.input_tokens, 0);
    }

    #[test]
    fn test_turn_output_with_token_usage() {
        let token_usage = argus_protocol::TokenUsage {
            input_tokens: 100,
            output_tokens: 50,
            total_tokens: 150,
        };
        let output = TurnOutputBuilder::default()
            .messages(Vec::new())
            .token_usage(token_usage.clone())
            .build()
            .unwrap();
        assert_eq!(output.token_usage, token_usage);
    }
}
