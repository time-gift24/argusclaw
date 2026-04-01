//! MCP repository trait.

use async_trait::async_trait;

use argus_protocol::{AgentId, AgentMcpBinding, McpDiscoveredToolRecord, McpServerRecord};

use crate::error::DbError;

/// Repository trait for MCP server and binding persistence.
#[async_trait]
pub trait McpRepository: Send + Sync {
    async fn upsert_mcp_server(&self, record: &McpServerRecord) -> Result<i64, DbError>;

    async fn get_mcp_server(&self, id: i64) -> Result<Option<McpServerRecord>, DbError>;

    async fn list_mcp_servers(&self) -> Result<Vec<McpServerRecord>, DbError>;

    async fn delete_mcp_server(&self, id: i64) -> Result<bool, DbError>;

    async fn replace_mcp_server_tools(
        &self,
        server_id: i64,
        tools: &[McpDiscoveredToolRecord],
    ) -> Result<(), DbError>;

    async fn list_mcp_server_tools(
        &self,
        server_id: i64,
    ) -> Result<Vec<McpDiscoveredToolRecord>, DbError>;

    async fn set_agent_mcp_bindings(
        &self,
        agent_id: AgentId,
        bindings: &[AgentMcpBinding],
    ) -> Result<(), DbError>;

    async fn list_agent_mcp_bindings(
        &self,
        agent_id: AgentId,
    ) -> Result<Vec<AgentMcpBinding>, DbError>;
}
