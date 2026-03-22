//! NamedTool adapter for MCP tools.

use std::sync::Arc;

use async_trait::async_trait;

use argus_protocol::llm::ToolDefinition;
use argus_protocol::risk_level::RiskLevel;
use argus_protocol::{NamedTool, ToolError};

use super::client::McpClientRuntime;

/// MCP tool adapter that wraps an MCP client tool as a NamedTool.
///
/// Each instance represents a single tool from an MCP server.
/// The tool name format is "mcp:{server_name}:{tool_name}".
pub struct McpTool {
    client: Arc<McpClientRuntime>,
    /// Original tool name from the MCP server.
    tool_name: String,
    /// Qualified name in format "mcp:{server_name}:{tool_name}".
    qualified_name: String,
    /// Original tool definition from the MCP server.
    definition: ToolDefinition,
}

impl McpTool {
    /// Create a new MCP tool adapter.
    ///
    /// # Arguments
    /// * `client` - The MCP client
    /// * `tool_name` - The name of the tool on the MCP server
    /// * `definition` - The tool definition from the MCP server
    #[must_use]
    pub fn new(
        client: Arc<McpClientRuntime>,
        tool_name: String,
        definition: ToolDefinition,
    ) -> Self {
        let server_name = client.server_name();
        let qualified_name = format!("mcp:{}:{}", server_name, tool_name);
        Self {
            client,
            tool_name,
            qualified_name,
            definition,
        }
    }
}

#[async_trait]
impl NamedTool for McpTool {
    fn name(&self) -> &str {
        &self.qualified_name
    }

    fn definition(&self) -> ToolDefinition {
        self.definition.clone()
    }

    fn risk_level(&self) -> RiskLevel {
        // MCP tools are untrusted external code, so Medium risk
        RiskLevel::Medium
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        self.client
            .call_tool(&self.tool_name, args)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool_name: self.name().to_string(),
                reason: e.to_string(),
            })
    }
}
