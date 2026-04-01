use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;

use argus_protocol::llm::ToolDefinition;
use argus_protocol::risk_level::RiskLevel;
use argus_protocol::{NamedTool, ToolError, ToolExecutionContext};

use super::error::ChromeToolError;
use super::installer::ChromePaths;
#[cfg(test)]
use super::installer::DriverDownloader;
#[cfg(test)]
use super::manager::BrowserBackend;
#[cfg(test)]
use super::manager::ChromeHost;
use super::manager::ChromeManager;
use super::models::{ChromeAction, ChromeToolArgs, OpenArgs};
use super::policy::ExplorePolicy;

const RO_ACTIONS: &[ChromeAction] = &[
    ChromeAction::Install,
    ChromeAction::Open,
    ChromeAction::Navigate,
    ChromeAction::Wait,
    ChromeAction::ExtractText,
    ChromeAction::ListLinks,
    ChromeAction::NetworkRequests,
    ChromeAction::GetDomSummary,
];

const INTERACTIVE_ACTIONS: &[ChromeAction] = &[
    ChromeAction::Install,
    ChromeAction::Open,
    ChromeAction::Navigate,
    ChromeAction::Wait,
    ChromeAction::ExtractText,
    ChromeAction::ListLinks,
    ChromeAction::NetworkRequests,
    ChromeAction::GetDomSummary,
    ChromeAction::Click,
    ChromeAction::Type,
    ChromeAction::GetUrl,
    ChromeAction::GetCookies,
];

pub struct ChromeTool {
    manager: Arc<ChromeManager>,
    policy: ExplorePolicy,
    interactive: bool,
}

impl Default for ChromeTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ChromeTool {
    #[must_use]
    pub fn new() -> Self {
        let paths = ChromePaths::from_home(&default_home_dir());
        Self {
            manager: Arc::new(ChromeManager::new_production(paths)),
            policy: ExplorePolicy::readonly(),
            interactive: false,
        }
    }

    #[must_use]
    pub fn new_interactive() -> Self {
        let paths = ChromePaths::from_home(&default_home_dir());
        Self {
            manager: Arc::new(ChromeManager::new_interactive_production(paths)),
            policy: ExplorePolicy::interactive(),
            interactive: true,
        }
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn new_for_test(backend: Arc<dyn BrowserBackend>) -> Self {
        Self::new_with_backend(backend)
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn new_with_managed_components_for_test(
        host: Arc<dyn ChromeHost>,
        downloader: Arc<dyn DriverDownloader>,
        paths: ChromePaths,
    ) -> Self {
        Self {
            manager: Arc::new(ChromeManager::new_with_managed_components_for_test(
                host, downloader, paths,
            )),
            policy: ExplorePolicy::readonly(),
            interactive: false,
        }
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn new_interactive_with_managed_components_for_test(
        host: Arc<dyn ChromeHost>,
        downloader: Arc<dyn DriverDownloader>,
        paths: ChromePaths,
    ) -> Self {
        Self {
            manager: Arc::new(
                ChromeManager::new_interactive_with_managed_components_for_test(
                    host, downloader, paths,
                ),
            ),
            policy: ExplorePolicy::interactive(),
            interactive: true,
        }
    }

    #[cfg(test)]
    fn new_with_backend(backend: Arc<dyn BrowserBackend>) -> Self {
        Self {
            manager: Arc::new(ChromeManager::new_for_test(backend)),
            policy: ExplorePolicy::readonly(),
            interactive: false,
        }
    }

    #[cfg(test)]
    pub(crate) fn new_interactive_with_backend(backend: Arc<dyn BrowserBackend>) -> Self {
        Self {
            manager: Arc::new(ChromeManager::new_for_test(backend)),
            policy: ExplorePolicy::interactive(),
            interactive: true,
        }
    }

    fn definition_parameters(interactive: bool) -> serde_json::Value {
        let actions: Vec<&str> = if interactive {
            INTERACTIVE_ACTIONS.iter().map(|a| a.as_str()).collect()
        } else {
            RO_ACTIONS.iter().map(|a| a.as_str()).collect()
        };

        let mut properties = json!({
            "action": {
                "type": "string",
                "enum": actions,
                "description": if interactive { "Browser action (includes interactive operations)" } else { "Read-only browser action" }
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
                "description": "CSS selector for element operations"
            },
            "timeout_ms": {
                "type": "integer",
                "description": "Optional bounded passive wait in milliseconds for wait"
            },
            "max_requests": {
                "type": "integer",
                "description": "Optional maximum number of request records for network requests"
            }
        });

        if interactive {
            properties
                .as_object_mut()
                .expect("properties is always an object")
                .insert(
                    "text".to_string(),
                    json!({"type": "string", "description": "Text to type into element (for type action)"}),
                );
        }

        json!({
            "type": "object",
            "properties": properties,
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
        let description = if self.interactive {
            "Chrome browser tool with interactive capabilities for explicit driver install, navigating OAuth2 login flows, typing credentials, clicking buttons, and extracting tokens."
                .to_string()
        } else {
            "Chrome explore tool for explicit driver install, opening pages, and inspecting page state."
                .to_string()
        };
        ToolDefinition {
            name: "chrome".to_string(),
            description,
            parameters: Self::definition_parameters(self.interactive),
        }
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Critical
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
            ChromeAction::Install => {
                let (detected, install) = self
                    .manager
                    .install_driver()
                    .await
                    .map_err(Self::map_error)?;

                Ok(json!({
                    "action": "install",
                    "browser_version": detected.browser_version,
                    "driver_version": install.driver_version,
                    "driver_path": install.patched_driver,
                    "cache_hit": install.cache_hit,
                }))
            }
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
            ChromeAction::Navigate => {
                let session_id = required_session_id(&args)?;
                let url = args.url.ok_or_else(|| ToolError::ExecutionFailed {
                    tool_name: "chrome".to_string(),
                    reason: "missing url for navigate action".to_string(),
                })?;
                let opened = self
                    .manager
                    .navigate(&session_id, &url)
                    .await
                    .map_err(Self::map_error)?;

                Ok(json!({
                    "action": "navigate",
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
                    "status": "ok",
                    "waited_ms": timeout_ms,
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
            ChromeAction::NetworkRequests => {
                let session_id = required_session_id(&args)?;
                let requests = self
                    .manager
                    .network_requests(&session_id, args.max_requests)
                    .await
                    .map_err(Self::map_error)?;

                Ok(json!({
                    "action": "network_requests",
                    "session_id": session_id,
                    "requests": requests,
                }))
            }
            ChromeAction::Click => {
                let session_id = required_session_id(&args)?;
                let selector = required_field(&args, "selector", args.selector.as_deref())?;
                self.manager
                    .click(&session_id, selector)
                    .await
                    .map_err(Self::map_error)?;
                Ok(json!({
                    "action": "click",
                    "session_id": session_id,
                    "status": "ok",
                }))
            }
            ChromeAction::Type => {
                let session_id = required_session_id(&args)?;
                let selector = required_field(&args, "selector", args.selector.as_deref())?;
                let text = required_field(&args, "text", args.text.as_deref())?;
                self.manager
                    .type_text(&session_id, selector, text)
                    .await
                    .map_err(Self::map_error)?;
                Ok(json!({
                    "action": "type",
                    "session_id": session_id,
                    "status": "ok",
                }))
            }
            ChromeAction::GetUrl => {
                let session_id = required_session_id(&args)?;
                let url = self
                    .manager
                    .current_url(&session_id)
                    .await
                    .map_err(Self::map_error)?;
                Ok(json!({
                    "action": "get_url",
                    "session_id": session_id,
                    "url": url,
                }))
            }
            ChromeAction::GetCookies => {
                let session_id = required_session_id(&args)?;
                let cookies = self
                    .manager
                    .get_cookies(&session_id)
                    .await
                    .map_err(Self::map_error)?;
                Ok(json!({
                    "action": "get_cookies",
                    "session_id": session_id,
                    "cookies": cookies,
                }))
            }
        }
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

fn required_field<'a>(
    args: &ChromeToolArgs,
    field_name: &str,
    value: Option<&'a str>,
) -> Result<&'a str, ToolError> {
    value.ok_or_else(|| ToolError::ExecutionFailed {
        tool_name: "chrome".to_string(),
        reason: format!("missing {field_name} for {} action", args.action.as_str()),
    })
}

pub(super) fn default_home_dir() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(std::path::PathBuf::from))
        .unwrap_or_else(std::env::temp_dir)
}
