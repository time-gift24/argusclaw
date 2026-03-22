//! MCP server persistence types.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use argus_protocol::mcp::{McpServerConfig, ServerType};

/// Strongly typed MCP server ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct McpServerId(i64);

impl McpServerId {
    #[must_use]
    pub fn new(id: i64) -> Self {
        Self(id)
    }

    #[must_use]
    pub fn into_inner(self) -> i64 {
        self.0
    }
}

impl From<i64> for McpServerId {
    fn from(id: i64) -> Self {
        Self::new(id)
    }
}

impl From<McpServerId> for i64 {
    fn from(id: McpServerId) -> Self {
        id.0
    }
}

/// MCP server record for persistence (includes auth token ciphertext).
#[derive(Debug, Clone)]
pub struct McpServerRecord {
    pub id: McpServerId,
    pub name: String,
    pub display_name: String,
    pub server_type: ServerType,
    pub url: Option<String>,
    pub headers: Option<HashMap<String, String>>,
    pub use_sse: bool,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub auth_token_ciphertext: Option<Vec<u8>>,
    pub auth_token_nonce: Option<Vec<u8>>,
    pub enabled: bool,
}

impl McpServerRecord {
    /// Convert to config without decrypted auth token.
    pub fn into_config(self) -> McpServerConfig {
        McpServerConfig {
            id: self.id.into_inner(),
            name: self.name,
            display_name: self.display_name,
            server_type: self.server_type,
            url: self.url,
            headers: self.headers,
            use_sse: self.use_sse,
            command: self.command,
            args: self.args,
            enabled: self.enabled,
        }
    }
}
