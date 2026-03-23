//! MCP client wrapper for connecting to MCP servers.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use argus_protocol::llm::ToolDefinition;
use argus_protocol::mcp::{McpServerConfig, ServerType};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::RwLock;

/// Error type for MCP client operations.
#[derive(Debug, thiserror::Error)]
pub enum McpClientError {
    #[error("Failed to create MCP client: {reason}")]
    CreationFailed { reason: String },

    #[error("Failed to list tools: {reason}")]
    ListToolsFailed { reason: String },

    #[error("Failed to call tool '{name}': {reason}")]
    CallToolFailed { name: String, reason: String },

    #[error("Unsupported transport type: {0}")]
    UnsupportedTransport(String),
}

/// Raw HTTP MCP client using reqwest with SSE transport.
pub struct McpClientRuntime {
    http_client: Client,
    url: String,
    headers: HashMap<String, String>,
    initialized: Arc<RwLock<bool>>,
    server_name: String,
}

impl McpClientRuntime {
    /// Create a new MCP client from server configuration.
    pub async fn new(config: &McpServerConfig) -> Result<Self, McpClientError> {
        let server_name = config.name.clone();

        match config.server_type {
            ServerType::Http => {
                let url = config
                    .url
                    .as_ref()
                    .ok_or_else(|| McpClientError::CreationFailed {
                        reason: "Missing URL for HTTP transport".to_string(),
                    })?;

                let headers = config.headers.as_ref().cloned().unwrap_or_default();

                let http_client = Client::builder()
                    .timeout(Duration::from_secs(60))
                    .build()
                    .map_err(|e| McpClientError::CreationFailed {
                        reason: format!("Failed to build HTTP client: {}", e),
                    })?;

                Ok(Self {
                    http_client,
                    url: url.clone(),
                    headers,
                    initialized: Arc::new(RwLock::new(false)),
                    server_name,
                })
            }
            ServerType::Stdio => Err(McpClientError::UnsupportedTransport(
                "Stdio transport not yet implemented".to_string(),
            )),
        }
    }

    /// Get the server name.
    pub fn server_name(&self) -> &str {
        &self.server_name
    }

    /// Ensure the client is initialized (MCP handshake).
    async fn ensure_initialized(&self) -> Result<(), McpClientError> {
        if *self.initialized.read().await {
            return Ok(());
        }

        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "1",
            "method": "initialize",
            "params": {
                "protocolVersion": "2024-11-05",
                "capabilities": {},
                "clientInfo": {
                    "name": "argus-tool",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }
        });

        let body = self.send_request(req).await?;
        let resp: JsonRpcResponse =
            serde_json::from_value(body.clone()).map_err(|e| McpClientError::CreationFailed {
                reason: format!("Invalid initialize response: {} -- {}", e, body),
            })?;

        if resp.error.is_some() {
            return Err(McpClientError::CreationFailed {
                reason: format!("Initialize error: {:?}", resp.error),
            });
        }

        // Send "initialized" notification
        let notif = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/initialized",
            "params": {}
        });
        let _ = self.send_request(notif).await;

        *self.initialized.write().await = true;
        Ok(())
    }

    /// Send a JSON-RPC request and parse the SSE response.
    async fn send_request(&self, body: Value) -> Result<Value, McpClientError> {
        let mut req = self
            .http_client
            .post(&self.url)
            .header("Content-Type", "application/json")
            .header("Accept", "application/json, text/event-stream")
            .json(&body);

        for (k, v) in &self.headers {
            req = req.header(k.as_str(), v.as_str());
        }

        let resp = req
            .send()
            .await
            .map_err(|e| McpClientError::CreationFailed {
                reason: format!("HTTP request failed: {}", e),
            })?;

        if !resp.status().is_success() {
            return Err(McpClientError::CreationFailed {
                reason: format!("HTTP error: {}", resp.status()),
            });
        }

        let content_type = resp
            .headers()
            .get("Content-Type")
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();

        let body_bytes = resp
            .bytes()
            .await
            .map_err(|e| McpClientError::CreationFailed {
                reason: format!("Failed to read response body: {}", e),
            })?;

        // Parse based on content type
        if content_type.contains("text/event-stream") {
            // Parse SSE response
            parse_sse_response(&body_bytes)
        } else {
            // Plain JSON response
            serde_json::from_slice(&body_bytes).map_err(|e| McpClientError::CreationFailed {
                reason: format!("Failed to parse JSON response: {}", e),
            })
        }
    }

    /// List all tools available from the MCP server.
    pub async fn list_tools(&self) -> Result<Vec<ToolDefinition>, McpClientError> {
        self.ensure_initialized().await?;

        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "2",
            "method": "tools/list",
            "params": {}
        });

        let body = self.send_request(req).await?;

        let resp: JsonRpcResponse =
            serde_json::from_value(body.clone()).map_err(|e| McpClientError::ListToolsFailed {
                reason: format!("Invalid response: {} -- {}", e, body),
            })?;

        let tools = resp
            .result
            .as_ref()
            .and_then(|r| r.get("tools"))
            .and_then(|t| serde_json::from_value::<Vec<ToolInfo>>(t.clone()).ok())
            .unwrap_or_default();

        let defs = tools
            .into_iter()
            .map(|tool| ToolDefinition {
                name: tool.name,
                description: tool.description.unwrap_or_default(),
                parameters: tool.input_schema,
            })
            .collect();

        Ok(defs)
    }

    /// Call a tool on the MCP server with the given arguments.
    pub async fn call_tool(&self, name: &str, args: Value) -> Result<Value, McpClientError> {
        self.ensure_initialized().await?;

        let req = serde_json::json!({
            "jsonrpc": "2.0",
            "id": "3",
            "method": "tools/call",
            "params": CallToolRequest {
                name: name.to_string(),
                arguments: args,
            }
        });

        let body = self.send_request(req).await?;

        let resp: JsonRpcResponse =
            serde_json::from_value(body.clone()).map_err(|e| McpClientError::CallToolFailed {
                name: name.to_string(),
                reason: format!("Invalid response: {} -- {}", e, body),
            })?;

        resp.result
            .map(convert_call_result)
            .ok_or_else(|| McpClientError::CallToolFailed {
                name: name.to_string(),
                reason: "No result in response".to_string(),
            })
    }
}

/// JSON-RPC response structure.
#[derive(Debug, Deserialize)]
struct JsonRpcResponse {
    #[serde(default)]
    jsonrpc: String,
    #[serde(default)]
    id: serde_json::Value,
    #[serde(default)]
    result: Option<Value>,
    #[serde(default)]
    error: Option<Value>,
}

/// Tool info from list_tools response.
#[derive(Debug, Deserialize)]
struct ToolInfo {
    name: String,
    description: Option<String>,
    #[serde(default)]
    input_schema: Value,
}

/// MCP JSON-RPC request params for tools/call.
#[derive(Serialize)]
struct CallToolRequest {
    name: String,
    arguments: Value,
}

/// Parse SSE response body and extract the JSON-RPC result.
/// The server returns: `id:1\n\nevent:message\n\ndata:{json}\n\n`
fn parse_sse_response(body: &[u8]) -> Result<Value, McpClientError> {
    let body_str = String::from_utf8_lossy(body);

    for line in body_str.lines() {
        let line = line.trim();
        if line.starts_with("data:") {
            let json_str = line.strip_prefix("data:").unwrap().trim();
            // Handle multi-line JSON (rare but possible)
            if json_str.is_empty() {
                continue;
            }
            return serde_json::from_str(json_str).map_err(|e| McpClientError::CreationFailed {
                reason: format!("Failed to parse SSE data JSON: {} -- {}", e, json_str),
            });
        }
    }

    // If no SSE format, try parsing as plain JSON
    serde_json::from_slice(body).map_err(|e| McpClientError::CreationFailed {
        reason: format!("Failed to parse response as JSON: {}", e),
    })
}

/// Convert the "result" part of a call_tool response to our output format.
fn convert_call_result(result: Value) -> Value {
    let content = result
        .get("content")
        .and_then(|c| c.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|block| {
                    let ty = block.get("type")?.as_str()?;
                    match ty {
                        "text" => Some(serde_json::json!({
                            "type": "text",
                            "text": block.get("text").and_then(|v| v.as_str()).unwrap_or("")
                        })),
                        "image" => Some(serde_json::json!({
                            "type": "image",
                            "data": block.get("data").and_then(|v| v.as_str()).unwrap_or(""),
                            "mimeType": block.get("mimeType").and_then(|v| v.as_str()).unwrap_or("")
                        })),
                        _ => None,
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let is_error = result
        .get("isError")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    serde_json::json!({
        "content": content,
        "isError": is_error
    })
}
