use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;

use argus_protocol::llm::ToolDefinition;
use argus_protocol::risk_level::RiskLevel;
use argus_protocol::{NamedTool, ToolError, ToolExecutionContext};

use super::error::ChromeToolError;
use super::manager::{BackendOpenResult, BrowserBackend, ChromeManager};
use super::models::{ChromeAction, ChromeToolArgs, OpenArgs};
use super::policy::ExplorePolicy;
use std::path::PathBuf;

const RO_ACTIONS: &[ChromeAction] = &[
    ChromeAction::Open,
    ChromeAction::Wait,
    ChromeAction::ExtractText,
    ChromeAction::ListLinks,
    ChromeAction::GetDomSummary,
    ChromeAction::Screenshot,
];

pub struct ChromeTool {
    manager: Arc<ChromeManager>,
    policy: ExplorePolicy,
}

impl Default for ChromeTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ChromeTool {
    #[must_use]
    pub fn new() -> Self {
        Self::new_with_backend(Arc::new(OfflineChromeBackend))
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn new_for_test(backend: Arc<dyn BrowserBackend>) -> Self {
        Self::new_with_backend(backend)
    }

    fn new_with_backend(backend: Arc<dyn BrowserBackend>) -> Self {
        Self {
            manager: Arc::new(ChromeManager::new_for_test(backend)),
            policy: ExplorePolicy::readonly(),
        }
    }

    fn definition_parameters() -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {
                "action": {
                    "type": "string",
                    "enum": RO_ACTIONS.iter().map(|action| action.as_str()).collect::<Vec<_>>(),
                    "description": "Read-only browser action"
                },
                "url": {
                    "type": "string",
                    "description": "Target URL for open"
                },
                "session_id": {
                    "type": "string",
                    "description": "Session ID returned by open"
                },
                "selector": {
                    "type": "string",
                    "description": "Optional CSS selector for scoped read actions"
                },
                "screenshot_path": {
                    "type": "string",
                    "description": "Output path for screenshot"
                },
                "timeout_ms": {
                    "type": "integer",
                    "description": "Optional timeout in milliseconds for wait"
                }
            },
            "required": ["action"],
            "additionalProperties": false
        })
    }

    fn map_error(error: ChromeToolError) -> ToolError {
        match error {
            ChromeToolError::ActionNotAllowed { action } => ToolError::NotAuthorized(action),
            other => ToolError::ExecutionFailed {
                tool_name: "chrome".to_string(),
                reason: other.to_string(),
            },
        }
    }
}

#[async_trait]
impl NamedTool for ChromeTool {
    fn name(&self) -> &str {
        "chrome"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "chrome".to_string(),
            description:
                "read-only Chrome explore tool for opening pages and inspecting page state."
                    .to_string(),
            parameters: Self::definition_parameters(),
        }
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::High
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: Arc<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError> {
        let args = ChromeToolArgs::validate(input).map_err(Self::map_error)?;
        self.policy
            .validate_action(args.action)
            .map_err(Self::map_error)?;

        match args.action {
            ChromeAction::Open => {
                let url = args.url.ok_or_else(|| ToolError::ExecutionFailed {
                    tool_name: "chrome".to_string(),
                    reason: "missing url for open action".to_string(),
                })?;
                let opened = self
                    .manager
                    .open(OpenArgs { url })
                    .await
                    .map_err(Self::map_error)?;

                Ok(json!({
                    "action": "open",
                    "session_id": opened.session_id,
                    "final_url": opened.final_url,
                    "page_title": opened.page_title,
                }))
            }
            ChromeAction::Wait => {
                let session_id = required_session_id(&args)?;
                self.manager
                    .session(&session_id)
                    .await
                    .map_err(Self::map_error)?;

                let timeout_ms = args.timeout_ms.unwrap_or(1).min(1_000);
                tokio::time::sleep(Duration::from_millis(timeout_ms)).await;
                Ok(json!({
                    "action": "wait",
                    "session_id": session_id,
                    "status": "ok"
                }))
            }
            ChromeAction::ExtractText => {
                let session_id = required_session_id(&args)?;
                let text = self
                    .manager
                    .extract_text(&session_id, args.selector.as_deref())
                    .await
                    .map_err(Self::map_error)?;

                Ok(json!({
                    "action": "extract_text",
                    "session_id": session_id,
                    "content": text,
                }))
            }
            ChromeAction::ListLinks => {
                let session_id = required_session_id(&args)?;
                let links = self
                    .manager
                    .list_links(&session_id)
                    .await
                    .map_err(Self::map_error)?;

                Ok(json!({
                    "action": "list_links",
                    "session_id": session_id,
                    "links": links,
                }))
            }
            ChromeAction::GetDomSummary => {
                let session_id = required_session_id(&args)?;
                let summary = self
                    .manager
                    .get_dom_summary(&session_id)
                    .await
                    .map_err(Self::map_error)?;

                Ok(json!({
                    "action": "get_dom_summary",
                    "session_id": session_id,
                    "summary": summary,
                }))
            }
            ChromeAction::Screenshot => {
                let session_id = required_session_id(&args)?;
                let screenshot_path =
                    args.screenshot_path
                        .ok_or_else(|| ToolError::ExecutionFailed {
                            tool_name: "chrome".to_string(),
                            reason: "missing screenshot_path for screenshot action".to_string(),
                        })?;
                let saved_path = self
                    .manager
                    .screenshot(&session_id, PathBuf::from(&screenshot_path))
                    .await
                    .map_err(Self::map_error)?;

                Ok(json!({
                    "action": "screenshot",
                    "session_id": session_id,
                    "screenshot_path": saved_path,
                    "status": "ok",
                }))
            }
            ChromeAction::Click => Err(ToolError::NotAuthorized(
                ChromeToolError::ActionNotAllowed {
                    action: ChromeAction::Click.as_str().to_string(),
                }
                .to_string(),
            )),
        }
    }
}

#[derive(Default)]
struct OfflineChromeBackend;

#[async_trait]
impl BrowserBackend for OfflineChromeBackend {
    async fn open(&self, _url: &str) -> Result<BackendOpenResult, ChromeToolError> {
        Err(ChromeToolError::InvalidArguments {
            reason: "chrome backend is not configured".to_string(),
        })
    }
}

fn required_session_id(args: &ChromeToolArgs) -> Result<String, ToolError> {
    args.session_id
        .clone()
        .ok_or_else(|| ToolError::ExecutionFailed {
            tool_name: "chrome".to_string(),
            reason: format!("missing session_id for {} action", args.action.as_str()),
        })
}
