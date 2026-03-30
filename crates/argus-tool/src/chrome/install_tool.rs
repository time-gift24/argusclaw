use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::json;

use argus_protocol::llm::ToolDefinition;
use argus_protocol::risk_level::RiskLevel;
use argus_protocol::{NamedTool, ToolError, ToolExecutionContext};

use super::installer::{ChromeInstaller, ChromePaths, DriverDownloader, ReqwestDriverDownloader};
use super::manager::{ChromeHost, SystemChromeHost};
use super::tool::default_home_dir;

pub struct ChromeInstallTool {
    host: Arc<dyn ChromeHost>,
    installer: Arc<ChromeInstaller>,
}

impl Default for ChromeInstallTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ChromeInstallTool {
    #[must_use]
    pub fn new() -> Self {
        let paths = ChromePaths::from_home(&default_home_dir());
        let host: Arc<dyn ChromeHost> = Arc::new(SystemChromeHost);
        let downloader: Arc<dyn DriverDownloader> = Arc::new(ReqwestDriverDownloader::new());
        Self {
            host,
            installer: Arc::new(ChromeInstaller::new(paths, downloader)),
        }
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn new_with_components_for_test(
        host: Arc<dyn ChromeHost>,
        downloader: Arc<dyn DriverDownloader>,
        paths: ChromePaths,
    ) -> Self {
        Self {
            host,
            installer: Arc::new(ChromeInstaller::new(paths, downloader)),
        }
    }

    fn parameters() -> serde_json::Value {
        json!({
            "type": "object",
            "properties": {},
            "additionalProperties": false
        })
    }

    fn map_error(error: impl ToString) -> ToolError {
        ToolError::ExecutionFailed {
            tool_name: "chrome_install".to_string(),
            reason: error.to_string(),
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ChromeInstallArgs {}

#[async_trait]
impl NamedTool for ChromeInstallTool {
    fn name(&self) -> &str {
        "chrome_install"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "chrome_install".to_string(),
            description:
                "Install or reuse the matching chromedriver for the locally installed Chrome browser."
                    .to_string(),
            parameters: Self::parameters(),
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
        let _: ChromeInstallArgs = serde_json::from_value(input).map_err(Self::map_error)?;
        let detected = self.host.discover_chrome().await.map_err(Self::map_error)?;
        let install = self
            .installer
            .ensure_driver(&detected.browser_version)
            .await
            .map_err(Self::map_error)?;

        Ok(json!({
            "browser_version": detected.browser_version,
            "driver_version": install.driver_version,
            "driver_path": install.patched_driver,
            "cache_hit": install.cache_hit,
        }))
    }
}
