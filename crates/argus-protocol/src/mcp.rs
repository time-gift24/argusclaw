//! MCP server configuration types.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

/// Server type for MCP server connection (standard MCP format).
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

/// Transport type alias for backward compatibility.
#[deprecated(since = "0.9.0", note = "Use ServerType instead")]
pub type TransportType = ServerType;

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
    /// Whether to use legacy SSE transport for HTTP servers.
    #[serde(default)]
    pub use_sse: bool,
    /// Command to execute for Stdio transport (e.g., "npx").
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    /// Arguments for Stdio transport (e.g., ["-y", "@modelcontextprotocol/server-filesystem", "/path"]).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    /// Whether this server is enabled.
    pub enabled: bool,
}

impl McpServerConfig {
    /// Create a new McpServerConfig with required fields.
    #[allow(deprecated)]
    pub fn new(id: i64, name: String, display_name: String, server_type: ServerType) -> Self {
        Self {
            id,
            name,
            display_name,
            server_type,
            url: None,
            headers: None,
            use_sse: false,
            command: None,
            args: None,
            enabled: true,
        }
    }

    /// Set the URL for HTTP transport.
    #[allow(deprecated)]
    pub fn with_url(mut self, url: String) -> Self {
        self.url = Some(url);
        self
    }

    /// Set the headers for HTTP transport.
    #[allow(deprecated)]
    pub fn with_headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers = Some(headers);
        self
    }

    /// Set whether HTTP transport should use SSE mode.
    #[allow(deprecated)]
    pub fn with_use_sse(mut self, use_sse: bool) -> Self {
        self.use_sse = use_sse;
        self
    }

    /// Set the command for Stdio transport.
    #[allow(deprecated)]
    pub fn with_command(mut self, command: String) -> Self {
        self.command = Some(command);
        self
    }

    /// Set the args for Stdio transport.
    #[allow(deprecated)]
    pub fn with_args(mut self, args: Vec<String>) -> Self {
        self.args = Some(args);
        self
    }

    /// Set the enabled flag.
    #[allow(deprecated)]
    pub fn with_enabled(mut self, enabled: bool) -> Self {
        self.enabled = enabled;
        self
    }
}

/// MCP server configuration for JSON serialization (excludes sensitive data).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfigJson {
    pub id: i64,
    pub name: String,
    pub display_name: String,
    pub server_type: ServerType,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub headers: Option<HashMap<String, String>>,
    #[serde(default)]
    pub use_sse: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub args: Option<Vec<String>>,
    pub enabled: bool,
}

impl From<McpServerConfig> for McpServerConfigJson {
    fn from(config: McpServerConfig) -> Self {
        Self {
            id: config.id,
            name: config.name,
            display_name: config.display_name,
            server_type: config.server_type,
            url: config.url,
            headers: config.headers,
            use_sse: config.use_sse,
            command: config.command,
            args: config.args,
            enabled: config.enabled,
        }
    }
}
