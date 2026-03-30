use std::fmt;
use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use thirtyfour::prelude::{By, WebDriver};
use tokio::process::Child;
use tokio::sync::Mutex;

use super::error::ChromeToolError;
use super::models::LinkSummary;

#[async_trait]
pub trait BrowserSession: Send + Sync {
    async fn extract_text(&self, selector: Option<&str>) -> Result<String, ChromeToolError>;
    async fn list_links(&self) -> Result<Vec<LinkSummary>, ChromeToolError>;
    async fn screenshot_png(&self) -> Result<Vec<u8>, ChromeToolError>;
    async fn shutdown(&self) -> Result<(), ChromeToolError>;
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
    driver: Mutex<Option<WebDriver>>,
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
        Self {
            driver: Mutex::new(Some(driver)),
            driver_process: Mutex::new(Some(driver_process)),
        }
    }

    async fn live_driver(&self) -> Result<WebDriver, ChromeToolError> {
        self.driver.lock().await.as_ref().cloned().ok_or_else(|| {
            ChromeToolError::SessionShutdownFailed {
                reason: "session already closed".to_string(),
            }
        })
    }
}

#[async_trait]
impl BrowserSession for ManagedWebDriverSession {
    async fn extract_text(&self, selector: Option<&str>) -> Result<String, ChromeToolError> {
        let driver = self.live_driver().await?;
        let element = match selector {
            Some(selector) => driver.find(By::Css(selector)).await,
            None => driver.find(By::Css("body")).await,
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
        let driver = self.live_driver().await?;
        let elements =
            driver
                .find_all(By::Css("a"))
                .await
                .map_err(|e| ChromeToolError::PageReadFailed {
                    reason: e.to_string(),
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
        let driver = self.live_driver().await?;
        driver
            .screenshot_as_png()
            .await
            .map_err(|e| ChromeToolError::ScreenshotFailed {
                reason: e.to_string(),
            })
    }

    async fn shutdown(&self) -> Result<(), ChromeToolError> {
        let driver_error = if let Some(driver) = self.driver.lock().await.take() {
            driver
                .quit()
                .await
                .err()
                .map(|e| ChromeToolError::SessionShutdownFailed {
                    reason: e.to_string(),
                })
        } else {
            None
        };

        let mut child_guard = self.driver_process.lock().await;
        let (result, child_closed) = finalize_shutdown(driver_error, child_guard.as_mut()).await;
        if child_closed {
            child_guard.take();
        }
        result
    }
}

impl Drop for ManagedWebDriverSession {
    fn drop(&mut self) {
        if let Ok(mut guard) = self.driver_process.try_lock()
            && let Some(child) = guard.as_mut()
        {
            let _ = child.start_kill();
        }
    }
}

pub(crate) async fn shutdown_child_process(child: &mut Child) -> Result<(), ChromeToolError> {
    match child.try_wait() {
        Ok(Some(_)) => Ok(()),
        Ok(None) => {
            child
                .start_kill()
                .map_err(|e| ChromeToolError::SessionShutdownFailed {
                    reason: e.to_string(),
                })?;
            child
                .wait()
                .await
                .map(|_| ())
                .map_err(|e| ChromeToolError::SessionShutdownFailed {
                    reason: e.to_string(),
                })
        }
        Err(e) => Err(ChromeToolError::SessionShutdownFailed {
            reason: e.to_string(),
        }),
    }
}

async fn finalize_shutdown(
    driver_error: Option<ChromeToolError>,
    child: Option<&mut Child>,
) -> (Result<(), ChromeToolError>, bool) {
    let child_error = match child {
        Some(child) => shutdown_child_process(child).await.err(),
        None => None,
    };
    let child_closed = child_error.is_none();

    (
        merge_shutdown_errors(driver_error, child_error),
        child_closed,
    )
}

fn merge_shutdown_errors(
    driver_error: Option<ChromeToolError>,
    child_error: Option<ChromeToolError>,
) -> Result<(), ChromeToolError> {
    match (driver_error, child_error) {
        (None, None) => Ok(()),
        (Some(error), None) | (None, Some(error)) => Err(error),
        (Some(driver_error), Some(child_error)) => Err(ChromeToolError::SessionShutdownFailed {
            reason: format!(
                "{}; child cleanup also failed: {}",
                shutdown_error_reason(driver_error),
                shutdown_error_reason(child_error)
            ),
        }),
    }
}

fn shutdown_error_reason(error: ChromeToolError) -> String {
    match error {
        ChromeToolError::SessionShutdownFailed { reason } => reason,
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::process::Stdio;

    use tokio::process::Command;

    use super::{ChromeToolError, finalize_shutdown};

    #[cfg(unix)]
    #[tokio::test]
    async fn finalize_shutdown_still_cleans_child_after_driver_quit_error() {
        let mut child = Command::new("sleep")
            .arg("30")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();

        let (result, child_closed) = finalize_shutdown(
            Some(ChromeToolError::SessionShutdownFailed {
                reason: "driver quit failed".to_string(),
            }),
            Some(&mut child),
        )
        .await;

        assert!(child_closed);
        assert!(
            matches!(result, Err(ChromeToolError::SessionShutdownFailed { reason }) if reason == "driver quit failed")
        );
        assert!(child.try_wait().unwrap().is_some());
    }
}
