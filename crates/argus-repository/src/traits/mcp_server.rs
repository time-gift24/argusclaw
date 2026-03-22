//! MCP server repository trait.

use async_trait::async_trait;

use crate::error::DbError;
use argus_protocol::McpServerConfig;

/// Repository trait for MCP server persistence.
#[async_trait]
pub trait McpServerRepository: Send + Sync {
    /// Create or update an MCP server configuration.
    async fn upsert(&self, config: &McpServerConfig) -> Result<(), DbError>;

    /// Get an MCP server by ID.
    async fn get(&self, id: i64) -> Result<Option<McpServerConfig>, DbError>;

    /// Get an MCP server by name.
    async fn get_by_name(&self, name: &str) -> Result<Option<McpServerConfig>, DbError>;

    /// List all MCP servers.
    async fn list(&self) -> Result<Vec<McpServerConfig>, DbError>;

    /// Delete an MCP server by ID.
    async fn delete(&self, id: i64) -> Result<bool, DbError>;
}
