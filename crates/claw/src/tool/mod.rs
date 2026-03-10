//! Tool module: defines NamedTool trait and ToolError for agent/LLM tool management.

use std::sync::Arc;

use async_trait::async_trait;
use dashmap::DashMap;

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
}

/// Registry for tools with concurrent access support.
pub struct ToolManager {
    tools: DashMap<String, Arc<dyn NamedTool>>,
}

impl ToolManager {
    #[must_use]
    pub fn new() -> Self {
        Self {
            tools: DashMap::new(),
        }
    }

    /// Register a tool. Overwrites if tool with same name already exists.
    pub fn register(&self, tool: Arc<dyn NamedTool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// Get a tool by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn NamedTool>> {
        self.tools.get(name).map(|v| v.clone())
    }

    /// List all registered tool definitions for LLM consumption.
    pub fn list_definitions(&self) -> Vec<ToolDefinition> {
        self.tools.iter().map(|e| e.definition()).collect()
    }

    /// Execute a tool by name with the provided arguments.
    pub async fn execute(
        &self,
        name: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, ToolError> {
        let tool = self.get(name).ok_or_else(|| ToolError::NotFound {
            id: name.to_string(),
        })?;
        tool.execute(args).await
    }
}

impl Default for ToolManager {
    fn default() -> Self {
        Self::new()
    }
}
