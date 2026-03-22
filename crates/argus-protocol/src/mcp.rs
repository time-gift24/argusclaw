//! MCP server configuration types.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Server type for MCP server connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ServerType {
    /// Standard I/O transport (for local MCP servers)
    Stdio,
    /// HTTP/SSE transport (for HTTP-based MCP servers)
    Http,
}

impl std::fmt::Display for ServerType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServerType::Stdio => write!(f, "stdio"),
            ServerType::Http => write!(f, "http"),
        }
    }
}

impl std::str::FromStr for ServerType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "stdio" => Ok(ServerType::Stdio),
            "http" => Ok(ServerType::Http),
            _ => Err(format!("Unknown server type: {}", s)),
        }
    }
}

/// MCP server configuration stored in database.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// Unique identifier (database auto-increment).
    pub id: i64,
    /// Unique server name used in tool naming (e.g., "filesystem").
    pub name: String,
    /// Display name for UI (e.g., "Filesystem MCP").
    pub display_name: String,
    /// Server type for connecting to this server.
    pub server_type: ServerType,
    /// URL for HTTP transport (e.g., "https://mcp.example.com/sse").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// HTTP headers for HTTP transport (e.g., Authorization).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
    /// Command to execute for Stdio transport (e.g., "npx").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Arguments for Stdio transport.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    /// Whether this server is enabled.
    pub enabled: bool,
}

impl McpServerConfig {
    /// Create a new McpServerConfig with required fields.
    pub fn new(id: i64, name: String, display_name: String, server_type: ServerType) -> Self {
        Self {
            id,
            name,
            display_name,
            server_type,
            url: None,
            headers: None,
            command: None,
            args: None,
            enabled: true,
        }
    }

    /// Set the URL for HTTP transport.
    pub fn with_url(mut self, url: String) -> Self {
        self.url = Some(url);
        self
    }

    /// Set the headers for HTTP transport.
    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Set the command for Stdio transport.
    pub fn with_command(mut self, command: String) -> Self {
        self.command = Some(command);
        self
    }

    /// Set the args for Stdio transport.
    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.args = Some(args);
        self
    }

    /// Set the enabled flag.
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// MCP server connection status.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "lowercase")]
pub enum McpServerStatus {
    /// Server is not connected.
    Disconnected,
    /// Connection is in progress.
    Connecting,
    /// Successfully connected with capabilities.
    Connected {
        /// List of tool names available from this server.
        tools: Vec<String>,
        /// When the connection was established.
        connected_at: DateTime<Utc>,
    },
    /// Connection failed with error.
    Failed {
        /// Error message describing the failure.
        error: String,
        /// When the connection failed.
        failed_at: DateTime<Utc>,
    },
}

/// MCP server capability information discovered during connection.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpCapabilityInfo {
    /// List of tool names available from this server.
    pub tools: Vec<String>,
    /// List of resource URIs available from this server.
    pub resources: Vec<String>,
    /// List of prompt names available from this server.
    pub prompts: Vec<String>,
}

