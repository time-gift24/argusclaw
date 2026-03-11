use async_trait::async_trait;
use crate::cookie::{get_cookies, CookieError};
use crate::tool::{NamedTool, ToolDefinition, ToolError};
use serde_json::json;

pub struct CookieTool;

#[async_trait]
impl NamedTool for CookieTool {
    fn name(&self) -> &str {
        "cookie_extractor"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "cookie_extractor".into(),
            description: "Extract cookies for a domain from Chrome via CDP. Requires Chrome running with --remote-debugging-port=9222".into(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "cdpUrl": {
                        "type": "string",
                        "description": "CDP WebSocket URL (e.g., ws://localhost:9222/devtools/browser/...)"
                    },
                    "domain": {
                        "type": "string",
                        "description": "Domain to filter cookies (e.g., example.com)"
                    }
                },
                "required": ["cdpUrl", "domain"]
            }),
        }
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let cdp_url = args["cdpUrl"]
            .as_str()
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool_name: "cookie_extractor".to_string(),
                reason: "Missing cdpUrl".into(),
            })?;

        let domain = args["domain"]
            .as_str()
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool_name: "cookie_extractor".to_string(),
                reason: "Missing domain".into(),
            })?;

        let cookies = get_cookies(cdp_url, domain).await
            .map_err(|e: CookieError| ToolError::ExecutionFailed {
                tool_name: "cookie_extractor".to_string(),
                reason: format!("Cookie extraction failed: {}", e),
            })?;

        Ok(json!({
            "domain": domain,
            "count": cookies.len(),
            "cookies": cookies
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cookie_tool_definition() {
        let tool = CookieTool;
        let def = tool.definition();

        assert_eq!(def.name, "cookie_extractor");
        assert!(def.description.contains("CDP"));
        assert!(def.description.contains("Chrome"));
        assert!(def.description.contains("--remote-debugging-port=9222"));

        // Verify input schema has required fields
        let params = def.parameters;
        assert_eq!(params["type"], "object");
        assert!(params["properties"]["cdpUrl"].is_object());
        assert!(params["properties"]["domain"].is_object());
        assert!(params["required"].is_array());
        let required: Vec<&str> = params["required"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|v| v.as_str())
            .collect();
        assert_eq!(required, vec!["cdpUrl", "domain"]);
    }

    #[test]
    fn test_cookie_tool_name() {
        let tool = CookieTool;
        assert_eq!(tool.name(), "cookie_extractor");
    }

    #[tokio::test]
    async fn test_cookie_tool_missing_cdp_url() {
        let tool = CookieTool;
        let args = json!({
            "domain": "example.com"
        });

        let result = tool.execute(args).await;
        assert!(result.is_err());
        match result {
            Err(ToolError::ExecutionFailed { tool_name, reason }) => {
                assert_eq!(tool_name, "cookie_extractor");
                assert!(reason.contains("Missing cdpUrl"));
            }
            _ => panic!("Expected ExecutionFailed error"),
        }
    }

    #[tokio::test]
    async fn test_cookie_tool_missing_domain() {
        let tool = CookieTool;
        let args = json!({
            "cdpUrl": "ws://localhost:9222/devtools/browser/abc"
        });

        let result = tool.execute(args).await;
        assert!(result.is_err());
        match result {
            Err(ToolError::ExecutionFailed { tool_name, reason }) => {
                assert_eq!(tool_name, "cookie_extractor");
                assert!(reason.contains("Missing domain"));
            }
            _ => panic!("Expected ExecutionFailed error"),
        }
    }

    #[tokio::test]
    async fn test_cookie_tool_registered_by_default() {
        use crate::tool::ToolManager;

        let manager = ToolManager::new();
        let tool = manager.get("cookie_extractor");

        assert!(tool.is_some());
        assert_eq!(tool.unwrap().name(), "cookie_extractor");
    }
}
