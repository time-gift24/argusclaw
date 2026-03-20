//! HTTP client tool.

use argus_protocol::http_client::HTTP_CLIENT;
use argus_protocol::llm::ToolDefinition;
use argus_protocol::risk_level::RiskLevel;
use argus_protocol::tool::{NamedTool, ToolError};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::Deserialize;
use url::Url;

const MAX_RESPONSE_SIZE: u64 = 10 * 1024 * 1024; // 10MB
const ALLOWED_METHODS: &[&str] = &["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD"];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HttpArgs {
    url: String,
    #[serde(default = "default_method")]
    method: String,
    #[serde(default)]
    headers: std::collections::HashMap<String, String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default = "default_timeout")]
    timeout: u64,
}

fn default_method() -> String {
    "GET".to_string()
}

fn default_timeout() -> u64 {
    30
}

#[derive(Debug, serde::Serialize)]
struct HttpResult {
    status: u16,
    status_text: String,
    headers: std::collections::HashMap<String, Vec<String>>,
    body: String,
}

pub struct HttpTool;

impl HttpTool {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for HttpTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NamedTool for HttpTool {
    fn name(&self) -> &str {
        "http"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "http".to_string(),
            description: "Make HTTP requests to any URL. Returns status, headers, and body."
                .to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "Target URL (http/https only)",
                    },
                    "method": {
                        "type": "string",
                        "description": "HTTP method (GET/POST/PUT/DELETE/PATCH/HEAD). Default: GET",
                        "default": "GET",
                    },
                    "headers": {
                        "type": "object",
                        "description": "HTTP headers as key-value pairs",
                        "additionalProperties": { "type": "string" },
                    },
                    "body": {
                        "type": "string",
                        "description": "Request body (sent as-is). Only for POST/PUT/PATCH.",
                    },
                    "timeout": {
                        "type": "integer",
                        "description": "Timeout in seconds. Default: 30, max: 300",
                        "default": 30,
                    },
                },
                "required": ["url"],
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Critical
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let args: HttpArgs =
            serde_json::from_value(args).map_err(|e| ToolError::ExecutionFailed {
                tool_name: "http".to_string(),
                reason: format!("invalid arguments: {e}"),
            })?;

        // -- URL validation --
        let parsed_url = Url::parse(&args.url).map_err(|e| ToolError::ExecutionFailed {
            tool_name: "http".to_string(),
            reason: format!("invalid URL: {e}"),
        })?;

        match parsed_url.scheme() {
            "http" | "https" => {}
            scheme => {
                return Err(ToolError::ExecutionFailed {
                    tool_name: "http".to_string(),
                    reason: format!(
                        "Unsupported URL scheme '{scheme}'. Only http and https are allowed."
                    ),
                });
            }
        }

        // -- Method validation --
        let method_upper = args.method.to_uppercase();
        if !ALLOWED_METHODS.contains(&method_upper.as_str()) {
            return Err(ToolError::ExecutionFailed {
                tool_name: "http".to_string(),
                reason: format!(
                    "Unsupported HTTP method: '{}'. Allowed: GET, POST, PUT, DELETE, PATCH, HEAD",
                    args.method
                ),
            });
        }

        let reqwest_method =
            reqwest::Method::from_bytes(method_upper.as_bytes()).map_err(|_| {
                ToolError::ExecutionFailed {
                    tool_name: "http".to_string(),
                    reason: format!("Unsupported HTTP method: '{}'", args.method),
                }
            })?;

        // -- Timeout clamping --
        let timeout_secs = args.timeout.clamp(1, 300);

        // -- Build headers --
        let mut header_map = HeaderMap::new();
        for (key, value) in &args.headers {
            let header_name =
                HeaderName::from_bytes(key.as_bytes()).map_err(|_| ToolError::ExecutionFailed {
                    tool_name: "http".to_string(),
                    reason: format!("invalid header name: {key}"),
                })?;
            let header_value =
                HeaderValue::from_str(value).map_err(|_| ToolError::ExecutionFailed {
                    tool_name: "http".to_string(),
                    reason: format!("invalid header value for '{key}'"),
                })?;
            header_map.insert(header_name, header_value);
        }

        // -- Build request --
        let client = HTTP_CLIENT.clone();
        let mut request = client.request(reqwest_method, args.url).headers(header_map);

        if let Some(body) = args.body {
            request = request.body(body);
        }

        request = request.timeout(std::time::Duration::from_secs(timeout_secs));

        // -- Send --
        let response = request
            .send()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool_name: "http".to_string(),
                reason: if e.is_timeout() {
                    format!("request timed out after {timeout_secs}s")
                } else {
                    format!("request failed: {e}")
                },
            })?;

        // -- Response size check --
        if let Some(len) = response.content_length()
            && len > MAX_RESPONSE_SIZE
        {
            return Err(ToolError::ExecutionFailed {
                tool_name: "http".to_string(),
                reason: "Response body too large (max 10MB)".to_string(),
            });
        }

        // -- Collect response --
        let status = response.status();
        let status_text = status.canonical_reason().unwrap_or("Unknown").to_string();

        let mut response_headers = std::collections::HashMap::new();
        for (key, value) in response.headers() {
            response_headers
                .entry(key.as_str().to_lowercase())
                .or_insert_with(Vec::new)
                .push(value.to_str().unwrap_or("").to_string());
        }

        let body = response
            .text()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool_name: "http".to_string(),
                reason: format!("failed to read response body: {e}"),
            })?;

        let result = HttpResult {
            status: status.as_u16(),
            status_text,
            headers: response_headers,
            body,
        };

        Ok(serde_json::to_value(result).expect("failed to serialize HTTP result"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_unsupported_scheme() {
        let tool = HttpTool::new();
        let result = tool
            .execute(serde_json::json!({
                "url": "file:///etc/passwd",
                "method": "GET"
            }))
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::ExecutionFailed { ref reason, .. }
            if reason.contains("Unsupported URL scheme")));
    }

    #[tokio::test]
    async fn test_unsupported_method() {
        let tool = HttpTool::new();
        let result = tool
            .execute(serde_json::json!({
                "url": "https://example.com",
                "method": "CONNECT"
            }))
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::ExecutionFailed { ref reason, .. }
            if reason.contains("Unsupported HTTP method")));
    }

    #[tokio::test]
    async fn test_invalid_url() {
        let tool = HttpTool::new();
        let result = tool
            .execute(serde_json::json!({
                "url": "not a valid url at all",
                "method": "GET"
            }))
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::ExecutionFailed { ref reason, .. }
            if reason.contains("invalid URL")));
    }

    #[test]
    fn tool_name_is_http() {
        let tool = HttpTool::new();
        assert_eq!(tool.name(), "http");
    }

    #[test]
    fn tool_risk_level_is_critical() {
        let tool = HttpTool::new();
        assert_eq!(tool.risk_level(), RiskLevel::Critical);
    }

    #[test]
    fn tool_definition_has_url_required() {
        let tool = HttpTool::new();
        let def = tool.definition();
        assert_eq!(def.name, "http");
        // Verify url is required by checking the schema contains "url" in required array
        let params = &def.parameters;
        let required = params.get("required").and_then(|r| r.as_array());
        assert!(required.map_or(false, |r| r.iter().any(|v| v == "url")));
    }
}
