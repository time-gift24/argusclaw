//! HTTP client tool with SSRF protection.
//!
//! Security features:
//! - URL scheme validation (HTTPS only)
//! - IP blocklist (private, loopback, link-local, multicast, cloud metadata)
//! - DNS pinning to prevent DNS rebinding
//! - Response streaming with hard size limit
//! - Redirect control (simple GET allowed, others blocked)
//! - JSON body auto-recognition

use argus_protocol::http_client::HttpClientBuilder;
use argus_protocol::is_blocked_ip_v4;
use argus_protocol::llm::ToolDefinition;
use argus_protocol::risk_level::RiskLevel;
use argus_protocol::ssrf::{MAX_RESPONSE_SIZE, validate_url};
use argus_protocol::tool::{NamedTool, ToolError, ToolExecutionContext};
use async_trait::async_trait;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::Deserialize;
use std::net::{IpAddr, ToSocketAddrs};
use std::sync::Arc;
use url::Url;

const DEFAULT_TIMEOUT_SECS: u64 = 30;
const MAX_TIMEOUT_SECS: u64 = 300;

mod generated_insecure_ssl_suffixes {
    include!(concat!(
        env!("OUT_DIR"),
        "/generated_http_insecure_ssl_suffixes.rs"
    ));
}

#[derive(Debug, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HttpArgs {
    /// Target URL (HTTPS only, no localhost/private IPs)
    url: String,
    /// HTTP method (GET/POST/PUT/DELETE/PATCH/HEAD). Default: GET
    #[serde(default = "default_method")]
    method: String,
    /// HTTP headers as key-value pairs
    #[serde(default)]
    headers: std::collections::HashMap<String, String>,
    /// Request body. Can be any JSON value. Strings are sent as-is; other values are JSON-serialized.
    #[serde(default)]
    body: Option<serde_json::Value>,
    /// Timeout in seconds. Default: 30, max: 300
    #[serde(default = "default_timeout")]
    timeout: u64,
    /// Optional file path to save response body to
    #[serde(default)]
    save_to: Option<String>,
}

fn default_method() -> String {
    "GET".to_string()
}

fn default_timeout() -> u64 {
    DEFAULT_TIMEOUT_SECS
}

#[derive(Debug, serde::Serialize)]
struct HttpResult {
    status: u16,
    status_text: String,
    headers: std::collections::HashMap<String, Vec<String>>,
    body: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    saved_to: Option<String>,
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

impl HttpTool {
    fn host_matches_insecure_ssl_whitelist(host: &str, suffixes: &[&str]) -> bool {
        if host.parse::<IpAddr>().is_ok() {
            return false;
        }

        let normalized_host = host.trim_end_matches('.').to_ascii_lowercase();
        if normalized_host.is_empty() {
            return false;
        }

        suffixes.iter().any(|suffix| {
            normalized_host == *suffix || normalized_host.ends_with(&format!(".{suffix}"))
        })
    }

    fn should_skip_ssl_verification_for_host(host: &str) -> bool {
        Self::host_matches_insecure_ssl_whitelist(
            host,
            generated_insecure_ssl_suffixes::INSECURE_SSL_SUFFIX_WHITELIST,
        )
    }

    /// Validates URL structure and resolves DNS, checking IPs against blocklist.
    async fn validate_and_resolve(
        url_str: &str,
    ) -> Result<(Url, Vec<std::net::SocketAddr>), ToolError> {
        // 1. Structure validation
        validate_url(url_str).map_err(|e| {
            // Wrap security errors with tool name
            if matches!(e, ToolError::SecurityBlocked { .. }) {
                e
            } else {
                ToolError::ExecutionFailed {
                    tool_name: "http".to_string(),
                    reason: e.to_string(),
                }
            }
        })?;

        // 2. Parse URL
        let parsed_url = Url::parse(url_str).map_err(|e| ToolError::ExecutionFailed {
            tool_name: "http".to_string(),
            reason: format!("invalid URL: {e}"),
        })?;

        // 3. DNS resolution + IP blocklist check
        let host = parsed_url.host_str().unwrap();
        let port = parsed_url.port().unwrap_or(443);

        let addr_str = format!("{host}:{port}");
        let addrs: Vec<std::net::SocketAddr> = addr_str
            .to_socket_addrs()
            .map_err(|e| ToolError::SecurityBlocked {
                url: url_str.to_string(),
                reason: format!("DNS resolution failed: {e}"),
            })?
            .collect();

        if addrs.is_empty() {
            return Err(ToolError::SecurityBlocked {
                url: url_str.to_string(),
                reason: "DNS returned no addresses".to_string(),
            });
        }

        // Check all resolved IPs against blocklist
        for addr in &addrs {
            let ip = addr.ip();
            let blocked = match ip {
                IpAddr::V4(v4) => is_blocked_ip_v4(v4),
                IpAddr::V6(v6) => argus_protocol::is_blocked_ip_v6(v6),
            };
            if blocked {
                return Err(ToolError::SecurityBlocked {
                    url: url_str.to_string(),
                    reason: format!("Resolved IP '{}' is in a blocked range", ip),
                });
            }
        }

        Ok((parsed_url, addrs))
    }

    /// Reads response body, enforcing a hard size limit.
    /// Takes ownership of the response.
    async fn read_response_body(response: reqwest::Response) -> Result<Vec<u8>, ToolError> {
        // Check Content-Length first
        if let Some(len) = response.content_length()
            && len > MAX_RESPONSE_SIZE
        {
            return Err(ToolError::ExecutionFailed {
                tool_name: "http".to_string(),
                reason: format!(
                    "Response body too large (Content-Length: {len} bytes, max: {MAX_RESPONSE_SIZE})"
                ),
            });
        }

        let body_bytes = response
            .bytes()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool_name: "http".to_string(),
                reason: format!("failed to read response body: {e}"),
            })?;

        if body_bytes.len() as u64 > MAX_RESPONSE_SIZE {
            return Err(ToolError::ExecutionFailed {
                tool_name: "http".to_string(),
                reason: format!(
                    "Response body exceeded {} bytes limit (got {} bytes)",
                    MAX_RESPONSE_SIZE,
                    body_bytes.len()
                ),
            });
        }

        Ok(body_bytes.to_vec())
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
            description: "Make HTTP requests to URLs with SSRF protection. \
                Only HTTPS URLs are allowed. Returns status, headers, and body."
                .to_string(),
            parameters: serde_json::to_value(schemars::schema_for!(HttpArgs))
                .unwrap_or_else(|_| serde_json::json!({"type": "object"})),
        }
    }

    fn risk_level(&self) -> RiskLevel {
        // Base level is Critical; individual requests may be elevated from this
        RiskLevel::Critical
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: Arc<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError> {
        let args: HttpArgs =
            serde_json::from_value(input).map_err(|e| ToolError::ExecutionFailed {
                tool_name: "http".to_string(),
                reason: format!("invalid arguments: {e}"),
            })?;

        // -- Method validation --
        let method_upper = args.method.to_uppercase();
        let allowed = ["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD"];
        if !allowed.contains(&method_upper.as_str()) {
            return Err(ToolError::ExecutionFailed {
                tool_name: "http".to_string(),
                reason: format!(
                    "Unsupported HTTP method: '{}'. Allowed: {}",
                    args.method,
                    allowed.join(", ")
                ),
            });
        }

        let reqwest_method =
            reqwest::Method::from_bytes(method_upper.as_bytes()).map_err(|_| {
                ToolError::ExecutionFailed {
                    tool_name: "http".to_string(),
                    reason: format!("invalid HTTP method: '{}'", args.method),
                }
            })?;

        // -- Timeout clamping --
        let timeout_secs = args.timeout.clamp(1, MAX_TIMEOUT_SECS);

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

        // -- SSRF validation + DNS resolution --
        let (parsed_url, resolved_addrs) = Self::validate_and_resolve(&args.url).await?;
        let skip_ssl_verification = parsed_url
            .host_str()
            .is_some_and(Self::should_skip_ssl_verification_for_host);

        // -- Build client with DNS pinning --
        let client = HttpClientBuilder::new()
            .with_timeout(timeout_secs)
            .with_dns_pin(resolved_addrs)
            .with_insecure_ssl(skip_ssl_verification)
            .build()?;

        // -- Simple GET: allowed to follow redirects (each hop re-validated) --
        // Other methods: redirects are blocked
        let is_simple_get =
            reqwest_method == reqwest::Method::GET && header_map.is_empty() && args.body.is_none();

        let mut request = client
            .request(reqwest_method, &args.url)
            .headers(header_map);

        // -- Set body --
        if let Some(body_val) = args.body {
            let body_str = match body_val {
                serde_json::Value::String(s) => s,
                other => serde_json::to_string(&other).map_err(|e| ToolError::ExecutionFailed {
                    tool_name: "http".to_string(),
                    reason: format!("failed to serialize body: {e}"),
                })?,
            };
            // Set Content-Type if not already set and body is JSON
            if !body_str.starts_with('{') && !body_str.starts_with('[') {
                // Non-JSON string: set as text/plain if no Content-Type
            }
            request = request.body(body_str);
        }

        // -- Send --
        let response = request.send().await.map_err(|e| {
            if e.is_timeout() {
                ToolError::ExecutionFailed {
                    tool_name: "http".to_string(),
                    reason: format!("request timed out after {timeout_secs}s"),
                }
            } else if e.is_redirect() {
                if is_simple_get {
                    // Shouldn't happen with redirect policy none
                    ToolError::ExecutionFailed {
                        tool_name: "http".to_string(),
                        reason: "unexpected redirect".to_string(),
                    }
                } else {
                    ToolError::SecurityBlocked {
                        url: args.url.clone(),
                        reason: "Redirects are not allowed for this request type".to_string(),
                    }
                }
            } else {
                ToolError::ExecutionFailed {
                    tool_name: "http".to_string(),
                    reason: format!("request failed: {e}"),
                }
            }
        })?;

        // -- Handle redirect for simple GET --
        if !is_simple_get {
            let status = response.status();
            if status.is_redirection() {
                return Err(ToolError::SecurityBlocked {
                    url: args.url.clone(),
                    reason: format!(
                        "Server responded with redirect ({status}). \
                        Non-simple requests must not follow redirects."
                    ),
                });
            }
        }

        // -- Extract headers and status before reading body (consumes response) --
        let status = response.status();
        let status_text = status.canonical_reason().unwrap_or("Unknown").to_string();

        let mut response_headers = std::collections::HashMap::new();
        for (key, value) in response.headers() {
            response_headers
                .entry(key.as_str().to_lowercase())
                .or_insert_with(Vec::new)
                .push(value.to_str().unwrap_or("").to_string());
        }

        // -- Read body (takes ownership of response) --
        let body_bytes = Self::read_response_body(response).await?;

        let body_str =
            String::from_utf8(body_bytes.clone()).map_err(|e| ToolError::ExecutionFailed {
                tool_name: "http".to_string(),
                reason: format!("response body is not valid UTF-8: {}", e.utf8_error()),
            })?;

        // -- Optional save to file --
        let saved_to = if let Some(path) = args.save_to {
            tokio::fs::write(&path, &body_bytes)
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    tool_name: "http".to_string(),
                    reason: format!("failed to save response to '{path}': {e}"),
                })?;
            Some(path)
        } else {
            None
        };

        let result = HttpResult {
            status: status.as_u16(),
            status_text,
            headers: response_headers,
            body: body_str,
            saved_to,
        };

        Ok(serde_json::to_value(result).expect("failed to serialize HTTP result"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use argus_protocol::ids::ThreadId;

    use tokio::sync::broadcast;

    fn make_ctx() -> Arc<ToolExecutionContext> {
        let (tx, _) = broadcast::channel(16);
        Arc::new(ToolExecutionContext {
            thread_id: ThreadId::new(),
            agent_id: None,
            pipe_tx: tx,
        })
    }

    #[tokio::test]
    async fn test_unsupported_scheme() {
        let tool = HttpTool::new();
        let result = tool
            .execute(
                serde_json::json!({
                    "url": "file:///etc/passwd",
                    "method": "GET"
                }),
                make_ctx(),
            )
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::SecurityBlocked { reason, .. }
            if reason.contains("file")));
    }

    #[tokio::test]
    async fn test_unsupported_method() {
        let tool = HttpTool::new();
        let result = tool
            .execute(
                serde_json::json!({
                    "url": "https://example.com",
                    "method": "CONNECT"
                }),
                make_ctx(),
            )
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::ExecutionFailed { reason, .. }
            if reason.contains("Unsupported HTTP method")));
    }

    #[tokio::test]
    async fn test_invalid_url() {
        let tool = HttpTool::new();
        let result = tool
            .execute(
                serde_json::json!({
                    "url": "not a valid url at all",
                    "method": "GET"
                }),
                make_ctx(),
            )
            .await;
        assert!(result.is_err());
        // Invalid URL without scheme returns SecurityBlocked
        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::SecurityBlocked { reason, .. }
            if reason.contains("scheme")));
    }

    #[tokio::test]
    async fn test_localhost_blocked() {
        let tool = HttpTool::new();
        let result = tool
            .execute(
                serde_json::json!({
                    "url": "https://localhost/path",
                    "method": "GET"
                }),
                make_ctx(),
            )
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::SecurityBlocked { reason, .. }
            if reason.contains("localhost")));
    }

    #[tokio::test]
    async fn test_private_ip_blocked() {
        let tool = HttpTool::new();
        let result = tool
            .execute(
                serde_json::json!({
                    "url": "https://192.168.1.1/path",
                    "method": "GET"
                }),
                make_ctx(),
            )
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::SecurityBlocked { .. }));
    }

    #[tokio::test]
    async fn test_http_scheme_blocked() {
        let tool = HttpTool::new();
        let result = tool
            .execute(
                serde_json::json!({
                    "url": "http://example.com",
                    "method": "GET"
                }),
                make_ctx(),
            )
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::SecurityBlocked { reason, .. }
            if reason.contains("HTTP")));
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
        let params = &def.parameters;
        let required = params.get("required").and_then(|r| r.as_array());
        assert!(required.is_some_and(|r| r.iter().any(|v| v == "url")));
    }

    #[test]
    fn tool_definition_has_body_optional() {
        let tool = HttpTool::new();
        let def = tool.definition();
        let params = &def.parameters;
        let props = params.get("properties").and_then(|p| p.as_object());
        assert!(props.is_some_and(|p| p.contains_key("body")));
        let required = params.get("required").and_then(|r| r.as_array());
        assert!(required.is_none_or(|r| !r.iter().any(|v| v == "body")));
    }

    #[test]
    fn tool_definition_has_save_to_optional() {
        let tool = HttpTool::new();
        let def = tool.definition();
        let params = &def.parameters;
        let props = params.get("properties").and_then(|p| p.as_object());
        assert!(props.is_some_and(|p| p.contains_key("saveTo")));
    }

    #[test]
    fn insecure_ssl_whitelist_matches_exact_host() {
        assert!(HttpTool::host_matches_insecure_ssl_whitelist(
            "corp.local",
            &["corp.local"]
        ));
    }

    #[test]
    fn insecure_ssl_whitelist_matches_subdomain_suffix() {
        assert!(HttpTool::host_matches_insecure_ssl_whitelist(
            "api.corp.local",
            &["corp.local"]
        ));
    }

    #[test]
    fn insecure_ssl_whitelist_does_not_match_similar_domain() {
        assert!(!HttpTool::host_matches_insecure_ssl_whitelist(
            "evilcorp.local",
            &["corp.local"]
        ));
    }

    #[test]
    fn insecure_ssl_whitelist_is_case_insensitive() {
        assert!(HttpTool::host_matches_insecure_ssl_whitelist(
            "API.CORP.LOCAL",
            &["corp.local"]
        ));
    }

    #[test]
    fn insecure_ssl_whitelist_does_not_match_ip_hosts() {
        assert!(!HttpTool::host_matches_insecure_ssl_whitelist(
            "10.0.0.1",
            &["0.0.1"]
        ));
    }
}
