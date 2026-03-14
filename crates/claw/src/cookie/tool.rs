//! Cookie tools for LLM/agent use.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::cookie::CookieManager;
use crate::llm::ToolDefinition;
use crate::protocol::RiskLevel;
use crate::tool::{NamedTool, ToolError};

/// Tool to retrieve cookies for a domain.
pub struct GetCookiesTool {
    manager: Arc<CookieManager>,
}

impl GetCookiesTool {
    /// Create new GetCookiesTool.
    #[must_use]
    pub fn new(manager: Arc<CookieManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl NamedTool for GetCookiesTool {
    fn name(&self) -> &str {
        "get_cookies"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "get_cookies".to_string(),
            description: "获取指定域名的 Cookie".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "domain": {
                        "type": "string",
                        "description": "目标域名，如 example.com"
                    }
                },
                "required": ["domain"]
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let domain = args["domain"]
            .as_str()
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool_name: "get_cookies".to_string(),
                reason: "Missing required parameter: domain".to_string(),
            })?;

        let cookies = self.manager.get_cookies(domain).await;

        let cookie_header = cookies
            .iter()
            .map(|c| format!("{}={}", c.name, c.value))
            .collect::<Vec<_>>()
            .join("; ");

        Ok(json!({
            "cookies": cookies,
            "cookie_header": cookie_header
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cookie::Cookie;

    async fn create_manager_with_cookie() -> Arc<CookieManager> {
        let manager = Arc::new(CookieManager::new());
        manager
            .add_cookie(Cookie {
                name: "session".into(),
                value: "abc123".into(),
                domain: "example.com".into(),
                path: "/".into(),
                secure: false,
                http_only: false,
                same_site: None,
                expires: None,
            })
            .await;
        manager
    }

    #[test]
    fn tool_metadata() {
        let manager = Arc::new(CookieManager::new());
        let tool = GetCookiesTool::new(manager);

        assert_eq!(tool.name(), "get_cookies");
        assert_eq!(tool.risk_level(), RiskLevel::Low);
        assert!(tool.definition().description.contains("Cookie"));
    }

    #[tokio::test]
    async fn get_cookies_returns_empty_for_unknown() {
        let manager = Arc::new(CookieManager::new());
        let tool = GetCookiesTool::new(manager);

        let result = tool
            .execute(json!({"domain": "unknown.com"}))
            .await
            .unwrap();
        assert_eq!(result["cookies"], json!([]));
        assert_eq!(result["cookie_header"], "");
    }

    #[tokio::test]
    async fn get_cookies_returns_cookies() {
        let manager = create_manager_with_cookie().await;
        let tool = GetCookiesTool::new(manager);

        let result = tool
            .execute(json!({"domain": "example.com"}))
            .await
            .unwrap();
        assert_eq!(result["cookies"].as_array().unwrap().len(), 1);
        assert_eq!(result["cookie_header"], "session=abc123");
    }

    #[tokio::test]
    async fn get_cookies_error_missing_domain() {
        let manager = Arc::new(CookieManager::new());
        let tool = GetCookiesTool::new(manager);

        let result = tool.execute(json!({})).await;
        assert!(result.is_err());
        match result {
            Err(ToolError::ExecutionFailed { tool_name, reason }) => {
                assert_eq!(tool_name, "get_cookies");
                assert!(reason.contains("domain"));
            }
            _ => panic!("Expected ExecutionFailed error"),
        }
    }
}
