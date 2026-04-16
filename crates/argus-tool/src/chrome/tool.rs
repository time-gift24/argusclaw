use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use serde::Serialize;
use serde_json::{Value, json};

use argus_protocol::llm::ToolDefinition;
use argus_protocol::risk_level::RiskLevel;
use argus_protocol::{NamedTool, ToolError, ToolExecutionContext};

use crate::serialize_tool_output;

use super::error::ChromeToolError;
use super::installer::ChromePaths;
#[cfg(test)]
use super::installer::DriverDownloader;
#[cfg(test)]
use super::manager::BrowserBackend;
#[cfg(test)]
use super::manager::ChromeHost;
use super::manager::ChromeManager;
use super::models::{ChromeAction, ChromeToolArgs};
use super::policy::ExplorePolicy;

const RO_ACTIONS: &[ChromeAction] = &[
    ChromeAction::Install,
    ChromeAction::Navigate,
    ChromeAction::Refresh,
    ChromeAction::Close,
    ChromeAction::Wait,
    ChromeAction::ExtractText,
    ChromeAction::NewTab,
    ChromeAction::SwitchTab,
    ChromeAction::CloseTab,
    ChromeAction::ListTabs,
];

const INTERACTIVE_ACTIONS: &[ChromeAction] = &[
    ChromeAction::Install,
    ChromeAction::Navigate,
    ChromeAction::Refresh,
    ChromeAction::Close,
    ChromeAction::Wait,
    ChromeAction::ExtractText,
    ChromeAction::Click,
    ChromeAction::Type,
    ChromeAction::GetUrl,
    ChromeAction::GetCookies,
    ChromeAction::NewTab,
    ChromeAction::SwitchTab,
    ChromeAction::CloseTab,
    ChromeAction::ListTabs,
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
    const TOOL_NAME: &'static str = "chrome";

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
    pub fn with_manager(manager: Arc<ChromeManager>) -> Self {
        Self {
            manager,
            policy: ExplorePolicy::readonly(),
            interactive: false,
        }
    }

    #[must_use]
    pub fn new_interactive_with_manager(manager: Arc<ChromeManager>) -> Self {
        Self {
            manager,
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
                "description": "Target URL for navigate or new_tab"
            },
            "selector": {
                "type": "string",
                "description": "CSS selector for element operations"
            },
            "timeout_ms": {
                "type": "integer",
                "description": "Optional bounded passive wait in milliseconds for wait"
            },
            "tab_id": {
                "type": "string",
                "description": "Tab identifier for tab operations (switch_tab, close_tab)"
            }
        });

        if interactive && let Some(map) = properties.as_object_mut() {
            map.insert(
                "text".to_string(),
                json!({"type": "string", "description": "Text to type into element (for type action)"}),
            );
            map.insert(
                "domain".to_string(),
                json!({"type": "string", "description": "Optional cookie domain filter for get_cookies"}),
            );
        }

        json!({
            "type": "object",
            "properties": properties,
            "required": ["action"],
            "additionalProperties": false
        })
    }

    fn serialize_response<T: Serialize>(value: T) -> Result<Value, ChromeToolError> {
        serialize_tool_output(Self::TOOL_NAME, value).map_err(ChromeToolError::from)
    }

    async fn execute_impl(&self, input: Value) -> Result<Value, ChromeToolError> {
        let args = ChromeToolArgs::validate(input)?;
        self.policy.validate_action(args.action)?;

        match args.action {
            ChromeAction::Install => {
                let (detected, install) = self.manager.install_driver().await?;
                Self::serialize_response(ChromeInstallResponse {
                    action: "install",
                    browser_version: detected.browser_version,
                    driver_version: install.driver_version,
                    driver_path: install.patched_driver,
                    cache_hit: install.cache_hit,
                })
            }
            ChromeAction::Navigate => {
                let url = required_field(&args, "url", args.url.as_deref())?;
                let opened = self.manager.navigate(url).await?;
                Self::serialize_response(ChromeNavigateResponse {
                    action: "navigate",
                    final_url: opened.final_url,
                    page_title: opened.page_title,
                })
            }
            ChromeAction::Refresh => {
                let opened = self.manager.refresh().await?;
                Self::serialize_response(ChromeNavigateResponse {
                    action: "refresh",
                    final_url: opened.final_url,
                    page_title: opened.page_title,
                })
            }
            ChromeAction::Close => {
                self.manager.close().await?;
                Self::serialize_response(ChromeStatusResponse {
                    action: "close",
                    status: "ok",
                })
            }
            ChromeAction::Wait => {
                self.manager.current_url().await?;

                let timeout_ms = args.timeout_ms.unwrap_or(1).min(1_000);
                tokio::time::sleep(Duration::from_millis(timeout_ms)).await;
                Self::serialize_response(ChromeWaitResponse {
                    action: "wait",
                    status: "ok",
                    waited_ms: timeout_ms,
                })
            }
            ChromeAction::ExtractText => {
                let text = self.manager.extract_text(args.selector.as_deref()).await?;
                Self::serialize_response(ChromeSessionContentResponse {
                    action: "extract_text",
                    content: text,
                })
            }
            ChromeAction::Click => {
                let selector = required_field(&args, "selector", args.selector.as_deref())?;
                self.manager.click(selector).await?;
                Self::serialize_response(ChromeSessionStatusResponse {
                    action: "click",
                    tab_id: None,
                    status: "ok",
                })
            }
            ChromeAction::Type => {
                let selector = required_field(&args, "selector", args.selector.as_deref())?;
                let text = required_field(&args, "text", args.text.as_deref())?;
                self.manager.type_text(selector, text).await?;
                Self::serialize_response(ChromeSessionStatusResponse {
                    action: "type",
                    tab_id: None,
                    status: "ok",
                })
            }
            ChromeAction::GetUrl => {
                let url = self.manager.current_url().await?;
                Self::serialize_response(ChromeSessionUrlResponse {
                    action: "get_url",
                    url,
                })
            }
            ChromeAction::GetCookies => {
                let cookies = self.manager.get_cookies(args.domain.as_deref()).await?;
                Self::serialize_response(ChromeCookiesResponse {
                    action: "get_cookies",
                    cookies,
                })
            }
            ChromeAction::NewTab => {
                let url = required_field(&args, "url", args.url.as_deref())?;
                let result = self.manager.new_tab(url).await?;
                Self::serialize_response(ChromeNewTabResponse {
                    action: "new_tab",
                    tab_id: result.tab_id,
                    url: result.url,
                    page_title: result.page_title,
                })
            }
            ChromeAction::SwitchTab => {
                let tab_id = required_field(&args, "tab_id", args.tab_id.as_deref())?;
                let metadata = self.manager.switch_tab(tab_id).await?;
                Self::serialize_response(ChromeTabMetadataResponse {
                    action: "switch_tab",
                    tab_id,
                    url: metadata.final_url,
                    page_title: metadata.page_title,
                })
            }
            ChromeAction::CloseTab => {
                let tab_id = required_field(&args, "tab_id", args.tab_id.as_deref())?;
                self.manager.close_tab(tab_id).await?;
                Self::serialize_response(ChromeSessionStatusResponse {
                    action: "close_tab",
                    tab_id: Some(tab_id),
                    status: "ok",
                })
            }
            ChromeAction::ListTabs => {
                let tabs = self.manager.list_tabs().await?;
                Self::serialize_response(ChromeListTabsResponse {
                    action: "list_tabs",
                    tabs,
                })
            }
        }
    }
}

impl From<ChromeToolError> for ToolError {
    fn from(error: ChromeToolError) -> Self {
        match error {
            ChromeToolError::ActionNotAllowed { action } => ToolError::NotAuthorized(action),
            other => ToolError::ExecutionFailed {
                tool_name: ChromeTool::TOOL_NAME.to_string(),
                reason: other.to_string(),
            },
        }
    }
}

#[derive(Debug, Serialize)]
struct ChromeInstallResponse {
    action: &'static str,
    browser_version: String,
    driver_version: String,
    driver_path: std::path::PathBuf,
    cache_hit: bool,
}

#[derive(Debug, Serialize)]
struct ChromeNavigateResponse {
    action: &'static str,
    final_url: String,
    page_title: String,
}

#[derive(Debug, Serialize)]
struct ChromeStatusResponse {
    action: &'static str,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct ChromeWaitResponse {
    action: &'static str,
    status: &'static str,
    waited_ms: u64,
}

#[derive(Debug, Serialize)]
struct ChromeSessionContentResponse {
    action: &'static str,
    content: String,
}

#[derive(Debug, Serialize)]
struct ChromeSessionStatusResponse<'a> {
    action: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    tab_id: Option<&'a str>,
    status: &'static str,
}

#[derive(Debug, Serialize)]
struct ChromeSessionUrlResponse {
    action: &'static str,
    url: String,
}

#[derive(Debug, Serialize)]
struct ChromeCookiesResponse<T> {
    action: &'static str,
    cookies: Vec<T>,
}

#[derive(Debug, Serialize)]
struct ChromeNewTabResponse {
    action: &'static str,
    tab_id: String,
    url: String,
    page_title: String,
}

#[derive(Debug, Serialize)]
struct ChromeTabMetadataResponse<'a> {
    action: &'static str,
    tab_id: &'a str,
    url: String,
    page_title: String,
}

#[derive(Debug, Serialize)]
struct ChromeListTabsResponse {
    action: &'static str,
    tabs: Vec<super::models::TabInfo>,
}

fn required_field<'a>(
    args: &ChromeToolArgs,
    field_name: &'static str,
    value: Option<&'a str>,
) -> Result<&'a str, ChromeToolError> {
    value.ok_or_else(|| ChromeToolError::MissingRequiredField {
        action: args.action.as_str().to_string(),
        field: field_name,
    })
}

#[async_trait]
impl NamedTool for ChromeTool {
    fn name(&self) -> &str {
        "chrome"
    }

    fn definition(&self) -> ToolDefinition {
        let description = if self.interactive {
            "Chrome browser tool with interactive capabilities for explicit driver install, a process-scoped browser session, navigate(url) for starting or switching pages, close for shutting down the browser, navigating OAuth2 login flows, typing credentials, clicking buttons, and extracting tokens."
                .to_string()
        } else {
            "Chrome explore tool for explicit driver install, a process-scoped browser session, navigate(url) for starting or switching pages, close for shutting down the browser, and inspecting page state."
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
        self.execute_impl(input).await.map_err(ToolError::from)
    }
}

pub(super) fn default_home_dir() -> std::path::PathBuf {
    std::env::var_os("HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::var_os("USERPROFILE").map(std::path::PathBuf::from))
        .unwrap_or_else(std::env::temp_dir)
}
