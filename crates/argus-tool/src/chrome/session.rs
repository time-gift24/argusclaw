use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use async_trait::async_trait;
use thirtyfour::prelude::{By, WebDriver};
use tokio::process::Child;

use super::error::ChromeToolError;
use super::models::LinkSummary;

#[async_trait]
pub trait BrowserSession: Send + Sync {
    async fn extract_text(&self, selector: Option<&str>) -> Result<String, ChromeToolError>;
    async fn list_links(&self) -> Result<Vec<LinkSummary>, ChromeToolError>;
    async fn screenshot_png(&self) -> Result<Vec<u8>, ChromeToolError>;
}

#[derive(Clone)]
pub struct ChromeSession {
    pub session_id: String,
    pub current_url: String,
    pub page_title: String,
    pub last_screenshot_path: Option<PathBuf>,
    interaction: Arc<dyn BrowserSession>,
}

impl fmt::Debug for ChromeSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChromeSession")
            .field("session_id", &self.session_id)
            .field("current_url", &self.current_url)
            .field("page_title", &self.page_title)
            .field("last_screenshot_path", &self.last_screenshot_path)
            .finish()
    }
}

impl ChromeSession {
    #[must_use]
    pub fn new(
        session_id: String,
        current_url: String,
        page_title: String,
        interaction: Arc<dyn BrowserSession>,
    ) -> Self {
        Self {
            session_id,
            current_url,
            page_title,
            last_screenshot_path: None,
            interaction,
        }
    }

    #[must_use]
    pub fn interaction(&self) -> Arc<dyn BrowserSession> {
        Arc::clone(&self.interaction)
    }

    pub fn set_last_screenshot_path(&mut self, path: Option<PathBuf>) {
        self.last_screenshot_path = path;
    }
}

pub struct ManagedWebDriverSession {
    driver: WebDriver,
    driver_process: Mutex<Option<Child>>,
}

impl fmt::Debug for ManagedWebDriverSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ManagedWebDriverSession")
            .finish_non_exhaustive()
    }
}

impl ManagedWebDriverSession {
    #[must_use]
    pub fn new(driver: WebDriver, driver_process: Child) -> Self {
        let _ = driver.clone().leak();
        Self {
            driver,
            driver_process: Mutex::new(Some(driver_process)),
        }
    }
}

#[async_trait]
impl BrowserSession for ManagedWebDriverSession {
    async fn extract_text(&self, selector: Option<&str>) -> Result<String, ChromeToolError> {
        let element = match selector {
            Some(selector) => self.driver.find(By::Css(selector)).await,
            None => self.driver.find(By::Css("body")).await,
        }
        .map_err(|e| ChromeToolError::PageReadFailed {
            reason: e.to_string(),
        })?;

        element
            .text()
            .await
            .map_err(|e| ChromeToolError::PageReadFailed {
                reason: e.to_string(),
            })
    }

    async fn list_links(&self) -> Result<Vec<LinkSummary>, ChromeToolError> {
        let elements = self.driver.find_all(By::Css("a")).await.map_err(|e| {
            ChromeToolError::PageReadFailed {
                reason: e.to_string(),
            }
        })?;
        let mut links = Vec::with_capacity(elements.len());
        for element in elements {
            let href = element
                .attr("href")
                .await
                .map_err(|e| ChromeToolError::PageReadFailed {
                    reason: e.to_string(),
                })?
                .unwrap_or_default();
            let text = element
                .text()
                .await
                .map_err(|e| ChromeToolError::PageReadFailed {
                    reason: e.to_string(),
                })?;
            links.push(LinkSummary { href, text });
        }
        Ok(links)
    }

    async fn screenshot_png(&self) -> Result<Vec<u8>, ChromeToolError> {
        self.driver
            .screenshot_as_png()
            .await
            .map_err(|e| ChromeToolError::ScreenshotFailed {
                reason: e.to_string(),
            })
    }
}

impl Drop for ManagedWebDriverSession {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.driver_process.lock() {
            if let Some(child) = guard.as_mut() {
                let _ = child.start_kill();
            }
        }
    }
}
