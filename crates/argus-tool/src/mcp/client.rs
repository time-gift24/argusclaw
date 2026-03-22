//! MCP client wrapper for connecting to MCP servers.

use std::sync::Arc;

use argus_protocol::llm::ToolDefinition;
use argus_protocol::mcp::{McpServerConfig, ServerType};
use rust_mcp_sdk::McpClient;
use rust_mcp_sdk::StreamableTransportOptions;
use rust_mcp_sdk::mcp_client::client_runtime::with_transport_options;
use rust_mcp_sdk::mcp_client::{ClientHandler, ClientRuntime};
use rust_mcp_sdk::schema::{CallToolRequestParams, ClientCapabilities, Implementation};
use serde_json::Value;
use tokio::sync::RwLock;

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

/// Client handler that implements ClientHandler with default behaviors.
#[derive(Clone)]
struct DefaultClientHandler;

impl ClientHandler for DefaultClientHandler {}

/// MCP client wrapper for connecting to MCP servers and calling their tools.
pub struct McpClientRuntime {
    runtime: Arc<RwLock<Option<Arc<ClientRuntime>>>>,
    server_name: String,
}

impl McpClientRuntime {
    /// Create a new MCP client from server configuration.
    pub async fn new(config: &McpServerConfig) -> Result<Self, McpClientError> {
        let server_name = config.name.clone();
        let runtime = Arc::new(RwLock::new(None));

        let runtime_inner = match config.server_type {
            ServerType::Http => {
                let url = config
                    .url
                    .as_ref()
                    .ok_or_else(|| McpClientError::CreationFailed {
                        reason: "Missing URL for HTTP transport".to_string(),
                    })?;

                Self::create_http_client(url).await?
            }
            ServerType::Stdio => {
                // Stdio transport requires more complex setup
                // For now, return an error indicating it's not yet supported
                return Err(McpClientError::UnsupportedTransport(
                    "Stdio transport not yet implemented".to_string(),
                ));
            }
        };

        {
            let mut guard = runtime.write().await;
            *guard = Some(runtime_inner.clone());
        }

        Ok(Self {
            runtime,
            server_name,
        })
    }

    /// Create an HTTP client with SSE transport.
    async fn create_http_client(url: &str) -> Result<Arc<ClientRuntime>, McpClientError> {
        let transport_options = StreamableTransportOptions {
            mcp_url: url.to_string(),
            request_options: Default::default(),
        };

        let client_details = rust_mcp_sdk::schema::InitializeRequestParams {
            protocol_version: rust_mcp_sdk::schema::ProtocolVersion::V2024_11_05.to_string(),
            capabilities: ClientCapabilities::default(),
            client_info: Implementation {
                name: "argus-tool".to_string(),
                version: env!("CARGO_PKG_VERSION").to_string(),
                description: Some("Argus tool MCP client".to_string()),
                icons: vec![],
                title: Some("Argus Tool".to_string()),
                website_url: None,
            },
            meta: None,
        };

        let handler = DefaultClientHandler;

        let runtime =
            with_transport_options(client_details, transport_options, handler, None, None, None);

        // Start the runtime
        let runtime_clone = runtime.clone();
        tokio::spawn(async move {
            if let Err(e) = runtime_clone.start().await {
                tracing::error!("MCP client runtime error: {}", e);
            }
        });

        Ok(runtime)
    }

    /// Get the server name.
    pub fn server_name(&self) -> &str {
        &self.server_name
    }

    /// Get the runtime, waiting for initialization if needed.
    async fn get_runtime(&self) -> Result<Arc<ClientRuntime>, McpClientError> {
        let guard = self.runtime.read().await;
        guard.clone().ok_or(McpClientError::NotInitialized)
    }

    /// List all tools available from the MCP server.
    pub async fn list_tools(&self) -> Result<Vec<ToolDefinition>, McpClientError> {
        let runtime = self.get_runtime().await?;

        // Wait for initialization
        let max_wait = std::time::Duration::from_secs(30);
        let start = std::time::Instant::now();
        while !runtime.is_initialized() {
            if start.elapsed() > max_wait {
                return Err(McpClientError::ListToolsFailed {
                    reason: "Timeout waiting for client initialization".to_string(),
                });
            }
            tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        }

        let result =
            runtime
                .request_tool_list(None)
                .await
                .map_err(|e| McpClientError::ListToolsFailed {
                    reason: e.to_string(),
                })?;

        // Convert Tool to ToolDefinition
        let tools = result
            .tools
            .into_iter()
            .map(|tool| convert_tool_to_definition(&tool))
            .collect();

        Ok(tools)
    }

    /// Call a tool on the MCP server with the given arguments.
    pub async fn call_tool(&self, name: &str, args: Value) -> Result<Value, McpClientError> {
        let runtime = self.get_runtime().await?;

        let params = CallToolRequestParams {
            name: name.to_string(),
            arguments: Some(serde_json::from_value(args).map_err(|e| {
                McpClientError::CallToolFailed {
                    name: name.to_string(),
                    reason: format!("Invalid arguments JSON: {}", e),
                }
            })?),
            meta: None,
            task: None,
        };

        let result = runtime.request_tool_call(params).await.map_err(|e| {
            McpClientError::CallToolFailed {
                name: name.to_string(),
                reason: e.to_string(),
            }
        })?;

        // Convert CallToolResult to JSON Value
        let output = convert_call_result(result);
        Ok(output)
    }
}

/// Convert a Tool from MCP schema to ToolDefinition for argus-protocol.
fn convert_tool_to_definition(tool: &rust_mcp_sdk::schema::Tool) -> ToolDefinition {
    ToolDefinition {
        name: tool.name.clone(),
        description: tool.description.clone().unwrap_or_default(),
        parameters: serde_json::to_value(&tool.input_schema).unwrap_or(serde_json::json!({})),
    }
}

/// Convert CallToolResult to JSON Value.
fn convert_call_result(result: rust_mcp_sdk::schema::CallToolResult) -> Value {
    let content = result
        .content
        .into_iter()
        .map(|block| match block {
            rust_mcp_sdk::schema::ContentBlock::TextContent(text_content) => {
                serde_json::json!({ "type": "text", "text": text_content.text })
            }
            rust_mcp_sdk::schema::ContentBlock::ImageContent(image_content) => {
                serde_json::json!({
                    "type": "image",
                    "data": image_content.data,
                    "mimeType": image_content.mime_type,
                })
            }
            rust_mcp_sdk::schema::ContentBlock::AudioContent(audio_content) => {
                serde_json::json!({
                    "type": "audio",
                    "data": audio_content.data,
                    "mimeType": audio_content.mime_type,
                })
            }
            rust_mcp_sdk::schema::ContentBlock::ResourceLink(link) => {
                serde_json::to_value(link).unwrap_or(serde_json::json!({}))
            }
            rust_mcp_sdk::schema::ContentBlock::EmbeddedResource(resource) => {
                serde_json::to_value(resource).unwrap_or(serde_json::json!({}))
            }
        })
        .collect::<Vec<_>>();

    let is_error = result.is_error.unwrap_or(false);

    serde_json::json!({
        "content": content,
        "isError": is_error
    })
}
