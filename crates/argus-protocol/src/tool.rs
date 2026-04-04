//! Tool types for agent/LLM tool management.
//!
//! This module defines the `NamedTool` trait and tool execution types
//! used across argus-* crates.

use async_trait::async_trait;

use std::sync::Arc;
use tokio::sync::broadcast;

use crate::ThreadEvent;
use crate::ids::{AgentId, ThreadId};
use crate::llm::ToolDefinition;
use crate::risk_level::RiskLevel;

/// Context passed to tools at execution time.
#[derive(Debug, Clone)]
pub struct ToolExecutionContext {
    /// The thread ID in which the tool is executing.
    pub thread_id: ThreadId,
    /// The agent ID that owns this thread, if available.
    pub agent_id: Option<AgentId>,
    /// The pipe sender for this thread. Tools can send ThreadEvent variants
    /// into this pipe. Failures are logged as warnings and do not block execution.
    pub pipe_tx: broadcast::Sender<ThreadEvent>,
}

/// Error type for tool operations.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// Tool not found in registry.
    #[error("Tool not found: {id}")]
    NotFound { id: String },

    /// Tool execution failed.
    #[error("Tool '{tool_name}' execution failed: {reason}")]
    ExecutionFailed { tool_name: String, reason: String },

    /// Request blocked by security policy (e.g., SSRF protection).
    #[error("HTTP request to '{url}' blocked: {reason}")]
    SecurityBlocked { url: String, reason: String },

    /// Operation not authorized.
    #[error("Operation not authorized: {0}")]
    NotAuthorized(String),

    /// Command timed out.
    #[error("Command timed out after {0:?}")]
    Timeout(std::time::Duration),
}

/// Trait for defining tools that can be used by agents and LLMs.
#[async_trait]
pub trait NamedTool: Send + Sync {
    /// Returns the unique name of the tool.
    fn name(&self) -> &str;

    /// Returns the tool definition for LLM consumption.
    fn definition(&self) -> ToolDefinition;

    /// Execute the tool with the provided arguments.
    ///
    /// `input` is the JSON arguments from the LLM.
    /// `ctx` provides execution context including the pipe for sending events.
    async fn execute(
        &self,
        input: serde_json::Value,
        ctx: Arc<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError>;

    /// Returns the risk level of this tool for approval gating.
    /// Default is `RiskLevel::Low` for read-only/safe operations.
    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }
}
