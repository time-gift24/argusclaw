//! MCP server repository trait.

use async_trait::async_trait;

use crate::error::DbError;
use crate::types::McpServerId;

/// Repository trait for MCP server persistence.
#[async_trait]
pub trait McpServerRepository: Send + Sync {
    /// List all MCP servers.
    async fn list(&self) -> Result<Vec<crate::types::McpServerRecord>, DbError>;

    /// Get an MCP server by ID.
    async fn get(&self, id: McpServerId) -> Result<Option<crate::types::McpServerRecord>, DbError>;

    /// Create a new MCP server.
    async fn create(&self, record: &crate::types::McpServerRecord) -> Result<McpServerId, DbError>;

    /// Update an existing MCP server.
    async fn update(&self, record: &crate::types::McpServerRecord) -> Result<(), DbError>;

    /// Delete an MCP server by ID.
    async fn delete(&self, id: McpServerId) -> Result<bool, DbError>;

    /// Get all enabled MCP servers.
    async fn list_enabled(&self) -> Result<Vec<crate::types::McpServerRecord>, DbError>;
}
