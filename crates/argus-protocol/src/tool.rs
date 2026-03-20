//! Tool types for agent/LLM tool management.
//!
//! This module contains shared types for tools used by argus-tool crate.

use async_trait::async_trait;

use crate::RiskLevel;
use crate::llm::ToolDefinition;

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
}

/// Trait for defining tools that can be used by agents and LLMs.
#[async_trait]
pub trait NamedTool: Send + Sync {
    /// Returns the unique name of the tool.
    fn name(&self) -> &str;

    /// Returns the tool definition for LLM consumption.
    fn definition(&self) -> ToolDefinition;

    /// Execute the tool with the provided arguments.
    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value, ToolError>;

    /// Returns the risk level of this tool for approval gating.
    /// Default is `RiskLevel::Low` for read-only/safe operations.
    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }
}
