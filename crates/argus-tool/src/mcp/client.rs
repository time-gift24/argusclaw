//! MCP client implementation using rust-mcp-sdk.

use super::mcp_error::{McpClientError, Result};

use std::sync::Arc;
use std::time::Duration;

use argus_protocol::llm::ToolDefinition;
use argus_protocol::mcp::{McpServerConfig, ServerType};
use argus_protocol::risk_level::RiskLevel;
use argus_protocol::{NamedTool, ToolError};
use async_trait::async_trait;
use dashmap::DashMap;
use rust_mcp_sdk::mcp_client::{
    ClientHandler, McpClientOptions, ToMcpClientHandler, client_runtime,
};
use rust_mcp_sdk::schema::{
    CallToolRequestParams, ClientCapabilities, Implementation, InitializeRequestParams,
    LATEST_PROTOCOL_VERSION,
};
use rust_mcp_sdk::{
    ClientSseTransport, ClientSseTransportOptions, McpClient, StdioTransport, TransportOptions,
};
use serde::{Deserialize, Serialize};
use tracing::{debug, info};

const MCP_CONNECTION_TEST_TIMEOUT_SECS: u64 = 2;

/// Connection test result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionTestResult {
    pub success: bool,
    pub tool_count: usize,
    pub error_message: Option<String>,
}

/// MCP client pool that manages connections to multiple MCP servers.
pub struct McpClientPool {
    /// Active client runtimes by server name.
    clients: DashMap<String, Arc<rust_mcp_sdk::mcp_client::ClientRuntime>>,
    /// Cached tool definitions by mcp tool name (e.g., "mcp_filesystem_read").
    tool_definitions: DashMap<String, ToolDefinition>,
}

/// MCP client wrapper for a specific server.
/// This wraps the runtime and provides tool listing/calling functionality.
pub struct McpClientWrapper {
    pub server_name: String,
    pub runtime: Arc<rust_mcp_sdk::mcp_client::ClientRuntime>,
    pub connected: bool,
}

impl McpClientWrapper {
    /// Create a new MCP client and connect to the server.
    pub async fn new(config: &McpServerConfig) -> Result<Self> {
        let server_name = config.name.clone();
        info!(
            "Creating MCP client for server '{}' ({:?})",
            server_name, config.server_type
        );

        // Create client details for initialization
        let client_details = InitializeRequestParams {
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: "argus-mcp-client".into(),
                version: env!("CARGO_PKG_VERSION").into(),
                title: Some("Argus MCP Client".into()),
                description: Some("Argus Wing MCP Client".into()),
                icons: vec![],
                website_url: None,
            },
            protocol_version: LATEST_PROTOCOL_VERSION.into(),
            meta: None,
        };

        // Create handler (using default implementation)
        let handler = EmptyClientHandler {};
        let handler_boxed = handler.to_mcp_client_handler();

        // Create and start the client runtime based on transport type
        let runtime: Arc<rust_mcp_sdk::mcp_client::ClientRuntime> =
            match config.server_type {
                ServerType::Stdio => {
                    let command =
                        config
                            .command
                            .as_ref()
                            .ok_or_else(|| McpClientError::InvalidConfig {
                                server: server_name.clone(),
                                reason: "Stdio transport requires command".to_string(),
                            })?;

                    // Parse command into program and args
                    let parts: Vec<&str> = command.split_whitespace().collect();
                    if parts.is_empty() {
                        return Err(McpClientError::InvalidConfig {
                            server: server_name.clone(),
                            reason: "Command is empty".to_string(),
                        });
                    }

                    let program = parts[0];
                    let args: Vec<String> = parts[1..].iter().map(|s| s.to_string()).collect();

                    let transport = StdioTransport::create_with_server_launch(
                        program,
                        args,
                        None,
                        TransportOptions::default(),
                    )
                    .map_err(|e| McpClientError::TransportError {
                        server: server_name.clone(),
                        reason: e.to_string(),
                    })?;

                    let options = McpClientOptions {
                        client_details,
                        transport,
                        handler: handler_boxed,
                        task_store: None,
                        server_task_store: None,
                        message_observer: None,
                    };

                    client_runtime::create_client(options)
                }
                ServerType::Http => {
                    let url = config
                        .url
                        .as_ref()
                        .ok_or_else(|| McpClientError::InvalidConfig {
                            server: server_name.clone(),
                            reason: "SSE transport requires url".to_string(),
                        })?;

                    let transport_options = ClientSseTransportOptions {
                        custom_headers: config.headers.clone(),
                        ..ClientSseTransportOptions::default()
                    };
                    let transport = ClientSseTransport::new(url.as_str(), transport_options)
                        .map_err(|e| McpClientError::TransportError {
                            server: server_name.clone(),
                            reason: e.to_string(),
                        })?;

                    let options = McpClientOptions {
                        client_details,
                        transport,
                        handler: handler_boxed,
                        task_store: None,
                        server_task_store: None,
                        message_observer: None,
                    };

                    client_runtime::create_client(options)
                }
            };

        // Start the runtime
        runtime
            .clone()
            .start()
            .await
            .map_err(|e| McpClientError::ConnectionFailed {
                server: server_name.clone(),
                reason: e.to_string(),
            })?;

        info!("Successfully connected to MCP server '{}'", server_name);

        Ok(Self {
            server_name,
            runtime,
            connected: true,
        })
    }

    /// Get the list of tools from this MCP server.
    pub async fn list_tools(&self) -> Result<Vec<ToolDefinition>> {
        let result = self.runtime.request_tool_list(None).await.map_err(|e| {
            McpClientError::ConnectionFailed {
                server: self.server_name.clone(),
                reason: e.to_string(),
            }
        })?;

        let tools: Vec<ToolDefinition> = result
            .tools
            .into_iter()
            .map(|tool| {
                // Convert the rust-mcp-sdk Tool to our ToolDefinition
                // The inputSchema is a JSON schema for the tool inputs
                ToolDefinition {
                    name: tool.name,
                    description: tool.description.unwrap_or_default(),
                    parameters: serde_json::to_value(&tool.input_schema).unwrap_or_else(|_| {
                        serde_json::json!({
                            "type": "object",
                            "properties": {}
                        })
                    }),
                }
            })
            .collect();

        Ok(tools)
    }

    /// Call a tool on this MCP server.
    pub async fn call_tool(
        &self,
        tool_name: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value> {
        // Convert args to Map if it's an object, otherwise use empty map
        let args_map = if let serde_json::Value::Object(map) = args {
            map
        } else {
            serde_json::Map::new()
        };

        let params = CallToolRequestParams {
            name: tool_name.to_string(),
            arguments: Some(args_map),
            meta: None,
            task: None,
        };

        let result = self.runtime.request_tool_call(params).await.map_err(|e| {
            McpClientError::ToolCallFailed {
                server: self.server_name.clone(),
                tool: tool_name.to_string(),
                reason: e.to_string(),
            }
        })?;

        // Convert CallToolResult to JSON value
        // The result contains a vec of content blocks
        let output = serde_json::to_value(&result).unwrap_or_else(|_| {
            serde_json::json!({
                "error": "Failed to serialize tool result"
            })
        });

        Ok(output)
    }
}

/// Empty client handler that uses default implementations for all methods.
pub struct EmptyClientHandler;

#[async_trait]
impl ClientHandler for EmptyClientHandler {}

impl McpClientPool {
    /// Create a new empty MCP client pool.
    #[must_use]
    pub fn new() -> Self {
        Self {
            clients: DashMap::new(),
            tool_definitions: DashMap::new(),
        }
    }

    /// Register MCP tools from a server config into the tool manager.
    /// Returns the number of tools registered, or an error.
    pub async fn register_server_tools(
        &self,
        config: &McpServerConfig,
        tool_manager: &crate::ToolManager,
    ) -> Result<usize> {
        let server_name = &config.name;

        info!(
            "Registering MCP server: {} ({:?})",
            server_name, config.server_type
        );

        // Create client and connect
        let client = match McpClientWrapper::new(config).await {
            Ok(c) => Arc::new(c),
            Err(e) => {
                return Err(McpClientError::ConnectionFailed {
                    server: server_name.clone(),
                    reason: e.to_string(),
                });
            }
        };

        // Discover tools from the server
        let tools = match client.list_tools().await {
            Ok(t) => t,
            Err(e) => {
                return Err(McpClientError::ConnectionFailed {
                    server: server_name.clone(),
                    reason: e.to_string(),
                });
            }
        };

        let tool_count = tools.len();
        debug!(
            "Discovered {} tools from MCP server '{}'",
            tool_count, server_name
        );

        // Register each tool with the tool manager
        for tool_def in &tools {
            let mcp_tool_name = format!("mcp_{}_{}", server_name, tool_def.name);
            let mcp_tool = McpTool::new(
                server_name.clone(),
                tool_def.name.clone(),
                Arc::clone(&client),
            );

            // Store the tool definition for later lookup
            let mut tool_def_with_name = tool_def.clone();
            tool_def_with_name.name = mcp_tool_name.clone();
            self.tool_definitions
                .insert(mcp_tool_name.clone(), tool_def_with_name);

            // Register in tool manager
            tool_manager.register(Arc::new(mcp_tool));

            debug!("Registered MCP tool: {}", mcp_tool_name);
        }

        // Store the client runtime
        self.clients
            .insert(server_name.clone(), Arc::clone(&client.runtime));

        info!(
            "Registered {} tools from MCP server '{}'",
            tool_count, server_name
        );

        Ok(tool_count)
    }

    /// Test connection to an MCP server.
    pub async fn test_connection(&self, config: &McpServerConfig) -> ConnectionTestResult {
        let server_name = &config.name;

        debug!("Testing connection to MCP server '{}'", server_name);

        // Validate configuration first
        match config.server_type {
            ServerType::Stdio if config.command.is_none() => {
                return ConnectionTestResult {
                    success: false,
                    tool_count: 0,
                    error_message: Some("Stdio transport requires command".to_string()),
                };
            }
            ServerType::Http if config.url.is_none() => {
                return ConnectionTestResult {
                    success: false,
                    tool_count: 0,
                    error_message: Some("SSE transport requires url".to_string()),
                };
            }
            _ => {}
        }

        let timeout = Duration::from_secs(MCP_CONNECTION_TEST_TIMEOUT_SECS);
        let test_future = async {
            let client = McpClientWrapper::new(config).await?;
            let tools = client.list_tools().await?;
            Ok::<usize, McpClientError>(tools.len())
        };

        match tokio::time::timeout(timeout, test_future).await {
            Ok(Ok(tool_count)) => ConnectionTestResult {
                success: true,
                tool_count,
                error_message: None,
            },
            Ok(Err(e)) => ConnectionTestResult {
                success: false,
                tool_count: 0,
                error_message: Some(e.to_string()),
            },
            Err(_) => ConnectionTestResult {
                success: false,
                tool_count: 0,
                error_message: Some(
                    McpClientError::Timeout {
                        server: server_name.clone(),
                        timeout_secs: MCP_CONNECTION_TEST_TIMEOUT_SECS,
                    }
                    .to_string(),
                ),
            },
        }
    }

    /// Get cached tool definition by mcp tool name.
    #[must_use]
    pub fn get_tool_definition(&self, mcp_tool_name: &str) -> Option<ToolDefinition> {
        self.tool_definitions.get(mcp_tool_name).map(|r| r.clone())
    }

    /// Check if a server is connected.
    #[must_use]
    pub fn is_server_connected(&self, server_name: &str) -> bool {
        self.clients.get(server_name).is_some()
    }
}

impl Default for McpClientPool {
    fn default() -> Self {
        Self::new()
    }
}

/// MCP tool that wraps a tool from an MCP server.
pub struct McpTool {
    server_name: String,
    tool_name: String,
    client: Arc<McpClientWrapper>,
}

impl McpTool {
    /// Create a new MCP tool.
    #[must_use]
    pub fn new(server_name: String, tool_name: String, client: Arc<McpClientWrapper>) -> Self {
        Self {
            server_name,
            tool_name,
            client,
        }
    }

    /// Get the MCP tool name (e.g., "mcp_filesystem_read").
    #[must_use]
    pub fn mcp_name(&self) -> String {
        format!("mcp_{}_{}", self.server_name, self.tool_name)
    }
}

#[async_trait]
impl NamedTool for McpTool {
    fn name(&self) -> &str {
        &self.tool_name
    }

    fn definition(&self) -> ToolDefinition {
        // Return a placeholder - in production, this would come from the server
        ToolDefinition {
            name: self.tool_name.clone(),
            description: format!("MCP tool '{}/{}'", self.server_name, self.tool_name),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {},
            }),
        }
    }

    async fn execute(
        &self,
        args: serde_json::Value,
    ) -> std::result::Result<serde_json::Value, ToolError> {
        debug!(
            "Executing MCP tool '{}/{}' with args: {:?}",
            self.server_name, self.tool_name, args
        );

        let result = self.client.call_tool(&self.tool_name, args).await;
        result.map_err(|e| e.into())
    }

    fn risk_level(&self) -> RiskLevel {
        // MCP tools default to Medium risk since they're external
        RiskLevel::Medium
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::time::Duration;

    #[tokio::test]
    async fn mcp_client_creation() {
        let config = McpServerConfig::new(
            1,
            "test".to_string(),
            "Test MCP".to_string(),
            ServerType::Stdio,
        )
        .with_command("npx -y test-server".to_string());

        // This will fail to connect but tests the validation path
        let result = McpClientWrapper::new(&config).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_connection_with_stdio() {
        let config = McpServerConfig::new(
            1,
            "test".to_string(),
            "Test MCP".to_string(),
            ServerType::Stdio,
        )
        .with_command("npx -y test-server".to_string());

        let pool = McpClientPool::new();
        let result = pool.test_connection(&config).await;
        // Will fail to connect, but tests the validation path
        assert!(!result.success || result.tool_count == 0);
    }

    #[tokio::test]
    async fn test_connection_missing_command() {
        let config = McpServerConfig::new(
            1,
            "test".to_string(),
            "Test MCP".to_string(),
            ServerType::Stdio,
        );

        let pool = McpClientPool::new();
        let result = pool.test_connection(&config).await;
        assert!(!result.success);
        assert!(result.error_message.unwrap().contains("command"));
    }

    #[tokio::test]
    async fn http_transport_validates_custom_headers() {
        let mut headers = HashMap::new();
        headers.insert("Invalid Header".to_string(), "value".to_string());

        let config = McpServerConfig::new(
            1,
            "test".to_string(),
            "Test MCP".to_string(),
            ServerType::Http,
        )
        .with_url("https://example.com/sse".to_string())
        .with_headers(headers);

        let result = McpClientWrapper::new(&config).await;
        assert!(result.is_err());
        let message = result.err().unwrap().to_string();
        assert!(message.contains("Invalid header name"));
    }

    #[tokio::test]
    async fn test_connection_returns_error_instead_of_blocking_forever() {
        let config = McpServerConfig::new(
            1,
            "test".to_string(),
            "Test MCP".to_string(),
            ServerType::Stdio,
        )
        .with_command("/bin/sleep 30".to_string());

        let pool = McpClientPool::new();
        let result =
            tokio::time::timeout(Duration::from_secs(3), pool.test_connection(&config)).await;

        assert!(
            result.is_ok(),
            "test_connection should return failure instead of blocking forever"
        );

        let test_result = result.unwrap();
        assert!(!test_result.success);
        assert!(
            test_result
                .error_message
                .unwrap_or_default()
                .contains("timed out")
        );
    }
}
