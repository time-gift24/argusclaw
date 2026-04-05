//! Turn configuration and stream event types.
//!
//! This module defines the configuration options and data structures used
//! for turn-based LLM conversation execution with tool support.

use std::sync::Arc;

use derive_builder::Builder;

use argus_protocol::{SafetyConfig, llm::LlmStreamEvent};

use super::TraceConfig;

// ---------------------------------------------------------------------------
// TurnConfig
// ---------------------------------------------------------------------------

/// Turn execution configuration.
///
/// Controls the behavior of a turn execution, including limits on tool calls,
/// timeouts, and iteration counts.
/// Callback invoked when a turn completes.
pub type OnTurnComplete = Arc<dyn Fn(argus_protocol::SessionId, u32) + Send + Sync>;

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

// ---------------------------------------------------------------------------
// ThreadConfig
// ---------------------------------------------------------------------------

/// Thread configuration.
#[derive(Debug, Clone, Builder)]
pub struct ThreadConfig {
    /// Underlying Turn configuration.
    #[builder(default)]
    pub turn_config: TurnConfig,
}

impl Default for ThreadConfig {
    fn default() -> Self {
        ThreadConfigBuilder::default()
            .build()
            .expect("ThreadConfigBuilder should not fail with defaults")
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

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
}
