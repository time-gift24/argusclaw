//! MCP client wrapper for connecting to MCP servers.

use std::sync::Arc;
use std::time::Duration;

use argus_protocol::llm::ToolDefinition;
use argus_protocol::mcp::{McpServerConfig, ServerType};
use pmcp::client::Client;
use pmcp::shared::http::{HttpConfig, HttpTransport};
use pmcp::types::capabilities::ClientCapabilities;
use pmcp::types::protocol::Implementation;
use serde_json::Value;
use tokio::sync::Mutex;

/// Error type for MCP client operations.
#[derive(Debug, thiserror::Error)]
pub enum McpClientError {
    /// Failed to create MCP client.
    #[error("Failed to create MCP client: {reason}")]
    CreationFailed { reason: String },

    /// Failed to list tools from MCP server.
    #[error("Failed to list tools: {reason}")]
    ListToolsFailed { reason: String },

    /// Failed to call tool on MCP server.
    #[error("Failed to call tool '{name}': {reason}")]
    CallToolFailed { name: String, reason: String },

    /// Client not initialized.
    #[error("Client not initialized")]
    NotInitialized,

    /// Unsupported transport type.
    #[error("Unsupported transport type: {0}")]
    UnsupportedTransport(String),
}

/// MCP client wrapper for connecting to MCP servers and calling their tools.
pub struct McpClientRuntime {
    client: Arc<Mutex<Client<HttpTransport>>>,
    server_name: String,
}

impl McpClientRuntime {
    /// Create a new MCP client from server configuration.
    pub async fn new(config: &McpServerConfig) -> Result<Self, McpClientError> {
        let server_name = config.name.clone();

        let client = match config.server_type {
            ServerType::Http => {
                let url = config
                    .url
                    .as_ref()
                    .ok_or_else(|| McpClientError::CreationFailed {
                        reason: "Missing URL for HTTP transport".to_string(),
                    })?;

                Self::create_http_client(url, config.headers.as_ref())?
            }
            ServerType::Stdio => {
                return Err(McpClientError::UnsupportedTransport(
                    "Stdio transport not yet implemented".to_string(),
                ));
            }
        };

        Ok(Self {
            client: Arc::new(Mutex::new(client)),
            server_name,
        })
    }

    /// Create an HTTP client with SSE transport.
    fn create_http_client(
        url: &str,
        headers: Option<&std::collections::HashMap<String, String>>,
    ) -> Result<Client<HttpTransport>, McpClientError> {
        use url::Url;

        let base_url = Url::parse(url).map_err(|e| McpClientError::CreationFailed {
            reason: format!("Invalid URL: {}", e),
        })?;

        let headers = headers
            .map(|h| {
                h.iter()
                    .map(|(k, v)| (k.clone(), v.clone()))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let http_config = HttpConfig {
            base_url,
            sse_endpoint: None,
            timeout: Duration::from_secs(60),
            headers,
            enable_pooling: true,
            max_idle_per_host: 5,
        };

        let transport = HttpTransport::new(http_config);

        let client_info = Implementation {
            name: "argus-tool".to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        };

        let client = Client::with_info(transport, client_info);

        Ok(client)
    }

    /// Get the server name.
    pub fn server_name(&self) -> &str {
        &self.server_name
    }

    /// List all tools available from the MCP server.
    pub async fn list_tools(&self) -> Result<Vec<ToolDefinition>, McpClientError> {
        let mut client = self.client.lock().await;
        self.ensure_initialized(&mut client).await?;

        let result =
            client
                .list_tools(None)
                .await
                .map_err(|e| McpClientError::ListToolsFailed {
                    reason: e.to_string(),
                })?;

        let tools = result
            .tools
            .into_iter()
            .map(|tool| ToolDefinition {
                name: tool.name,
                description: tool.description.unwrap_or_default(),
                parameters: tool.input_schema,
            })
            .collect();

        Ok(tools)
    }

    /// Call a tool on the MCP server with the given arguments.
    pub async fn call_tool(&self, name: &str, args: Value) -> Result<Value, McpClientError> {
        let mut client = self.client.lock().await;
        self.ensure_initialized(&mut client).await?;

        let result = client
            .call_tool(name.to_string(), args)
            .await
            .map_err(|e| McpClientError::CallToolFailed {
                name: name.to_string(),
                reason: e.to_string(),
            })?;

        let output = convert_call_result(result);
        Ok(output)
    }

    /// Ensure the client is initialized (MCP handshake).
    async fn ensure_initialized(
        &self,
        client: &mut Client<HttpTransport>,
    ) -> Result<(), McpClientError> {
        client
            .initialize(ClientCapabilities::default())
            .await
            .map_err(|e| McpClientError::CreationFailed {
                reason: format!("MCP initialization failed: {}", e),
            })?;
        Ok(())
    }
}

/// Convert CallToolResult to JSON Value.
fn convert_call_result(result: pmcp::types::protocol::CallToolResult) -> Value {
    let content = result
        .content
        .into_iter()
        .map(|block| match block {
            pmcp::types::protocol::Content::Text { text } => {
                serde_json::json!({ "type": "text", "text": text })
            }
            pmcp::types::protocol::Content::Image { data, mime_type } => {
                serde_json::json!({
                    "type": "image",
                    "data": data,
                    "mimeType": mime_type,
                })
            }
            pmcp::types::protocol::Content::Resource {
                uri,
                text,
                mime_type,
            } => {
                serde_json::json!({
                    "type": "resource",
                    "uri": uri,
                    "text": text,
                    "mimeType": mime_type,
                })
            }
        })
        .collect::<Vec<_>>();

    serde_json::json!({
        "content": content,
        "isError": result.is_error,
    })
}
