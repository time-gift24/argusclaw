use std::collections::{HashMap, HashSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use thirtyfour::common::cookie::Cookie;
use thirtyfour::prelude::{By, WebDriver};
use tokio::process::Child;
use tokio::sync::Mutex;

use super::error::ChromeToolError;
use super::models::PageMetadata;

#[async_trait]
pub trait BrowserSession: Send + Sync {
    async fn extract_text(&self, selector: Option<&str>) -> Result<String, ChromeToolError>;
    async fn shutdown(&self) -> Result<(), ChromeToolError>;
    async fn click(&self, selector: &str) -> Result<(), ChromeToolError>;
    async fn type_text(&self, selector: &str, text: &str) -> Result<(), ChromeToolError>;
    async fn current_url(&self) -> Result<String, ChromeToolError>;
    async fn get_cookies(&self) -> Result<Vec<Cookie>, ChromeToolError>;
    async fn navigate(&self, url: &str) -> Result<PageMetadata, ChromeToolError>;
    async fn create_new_tab(&self, url: &str) -> Result<(String, PageMetadata), ChromeToolError>;
    async fn switch_to_window(&self, window_handle: &str) -> Result<PageMetadata, ChromeToolError>;
    async fn close_current_window(&self) -> Result<(), ChromeToolError>;
    async fn list_windows(&self) -> Result<Vec<(String, String, String)>, ChromeToolError>;
    async fn current_window_handle(&self) -> Result<String, ChromeToolError>;
}

struct TabEntry {
    window_handle: String,
    url: String,
    title: String,
}

#[derive(Default)]
struct TabState {
    tabs: HashMap<String, TabEntry>,
    handle_to_tab_id: HashMap<String, String>,
    active_tab_id: Option<String>,
}

#[derive(Clone)]
pub struct ChromeSession {
    pub session_id: String,
    pub current_url: String,
    pub page_title: String,
    interaction: Arc<dyn BrowserSession>,
    tab_state: Arc<Mutex<TabState>>,
    next_tab_counter: Arc<AtomicU64>,
}

impl fmt::Debug for ChromeSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChromeSession")
            .field("session_id", &self.session_id)
            .field("current_url", &self.current_url)
            .field("page_title", &self.page_title)
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
            interaction,
            tab_state: Arc::new(Mutex::new(TabState::default())),
            next_tab_counter: Arc::new(AtomicU64::new(0)),
        }
    }

    #[must_use]
    pub fn interaction(&self) -> Arc<dyn BrowserSession> {
        Arc::clone(&self.interaction)
    }

    pub fn update_metadata(&mut self, metadata: PageMetadata) {
        self.current_url = metadata.final_url;
        self.page_title = metadata.page_title;
    }

    pub async fn register_initial_tab(&self, window_handle: String) {
        let mut state = self.tab_state.lock().await;
        let tab_id = self.ensure_tab_for_window(
            &mut state,
            window_handle,
            self.current_url.clone(),
            self.page_title.clone(),
        );
        state.active_tab_id = Some(tab_id);
    }

    pub async fn create_new_tab(
        &self,
        url: &str,
    ) -> Result<super::models::NewTabResult, ChromeToolError> {
        let (window_handle, metadata) = self.interaction.create_new_tab(url).await?;
        let tab_id = self.allocate_tab_id();
        let mut state = self.tab_state.lock().await;
        state.tabs.insert(
            tab_id.clone(),
            TabEntry {
                window_handle: window_handle.clone(),
                url: metadata.final_url.clone(),
                title: metadata.page_title.clone(),
            },
        );
        state.handle_to_tab_id.insert(window_handle, tab_id.clone());
        state.active_tab_id = Some(tab_id.clone());
        Ok(super::models::NewTabResult {
            tab_id,
            url: metadata.final_url,
            page_title: metadata.page_title,
        })
    }

    pub async fn switch_tab(&self, tab_id: &str) -> Result<PageMetadata, ChromeToolError> {
        let window_handle = self
            .tab_state
            .lock()
            .await
            .tabs
            .get(tab_id)
            .map(|entry| entry.window_handle.clone())
            .ok_or_else(|| ChromeToolError::TabNotFound {
                tab_id: tab_id.to_string(),
            })?;

        let metadata = self.interaction.switch_to_window(&window_handle).await?;
        {
            let mut state = self.tab_state.lock().await;
            if let Some(entry) = state.tabs.get_mut(tab_id) {
                entry.url = metadata.final_url.clone();
                entry.title = metadata.page_title.clone();
            }
            state.active_tab_id = Some(tab_id.to_string());
        }
        Ok(metadata)
    }

    pub async fn close_tab(&self, tab_id: &str) -> Result<PageMetadata, ChromeToolError> {
        let (window_handle, was_active, previous_active_handle) = {
            let state = self.tab_state.lock().await;
            if state.tabs.len() <= 1 {
                return Err(ChromeToolError::CannotCloseLastTab {
                    session_id: self.session_id.clone(),
                });
            }
            let entry = state
                .tabs
                .get(tab_id)
                .ok_or_else(|| ChromeToolError::TabNotFound {
                    tab_id: tab_id.to_string(),
                })?;
            let previous_active_handle = state
                .active_tab_id
                .as_deref()
                .and_then(|active_id| state.tabs.get(active_id))
                .map(|entry| entry.window_handle.clone());
            (
                entry.window_handle.clone(),
                state.active_tab_id.as_deref() == Some(tab_id),
                previous_active_handle,
            )
        };

        self.interaction.switch_to_window(&window_handle).await?;
        self.interaction.close_current_window().await?;

        if !was_active && let Some(previous_active_handle) = previous_active_handle.as_deref() {
            self.interaction
                .switch_to_window(previous_active_handle)
                .await?;
        }

        let active_window_handle = self.interaction.current_window_handle().await?;
        let metadata = self
            .interaction
            .switch_to_window(&active_window_handle)
            .await?;

        let mut state = self.tab_state.lock().await;
        self.remove_tab(&mut state, tab_id);
        let active_tab_id = self.ensure_tab_for_window(
            &mut state,
            active_window_handle,
            metadata.final_url.clone(),
            metadata.page_title.clone(),
        );
        state.active_tab_id = Some(active_tab_id);
        Ok(metadata)
    }

    pub async fn list_tabs(&self) -> Result<Vec<super::models::TabInfo>, ChromeToolError> {
        let windows = self.interaction.list_windows().await?;
        let active_window_handle = self.interaction.current_window_handle().await.ok();

        let mut state = self.tab_state.lock().await;
        self.reconcile_windows(&mut state, &windows);

        if let Some(active_window_handle) = active_window_handle
            && let Some(active_tab_id) = state.handle_to_tab_id.get(&active_window_handle).cloned()
        {
            state.active_tab_id = Some(active_tab_id);
        }

        let active_tab_id = state.active_tab_id.clone();
        let mut result = Vec::with_capacity(windows.len());
        for (handle, url, title) in windows {
            let tab_id = state
                .handle_to_tab_id
                .get(&handle)
                .cloned()
                .ok_or_else(|| ChromeToolError::TabOperationFailed {
                    reason: format!("missing tab mapping for window '{handle}'"),
                })?;
            result.push(super::models::TabInfo {
                active: active_tab_id.as_ref() == Some(&tab_id),
                tab_id,
                url,
                title,
            });
        }
        Ok(result)
    }

    fn ensure_tab_for_window(
        &self,
        state: &mut TabState,
        window_handle: String,
        url: String,
        title: String,
    ) -> String {
        if let Some(tab_id) = state.handle_to_tab_id.get(&window_handle).cloned() {
            state.tabs.insert(
                tab_id.clone(),
                TabEntry {
                    window_handle,
                    url,
                    title,
                },
            );
            return tab_id;
        }

        let tab_id = self.allocate_tab_id();
        state.tabs.insert(
            tab_id.clone(),
            TabEntry {
                window_handle: window_handle.clone(),
                url,
                title,
            },
        );
        state.handle_to_tab_id.insert(window_handle, tab_id.clone());
        tab_id
    }

    fn reconcile_windows(&self, state: &mut TabState, windows: &[(String, String, String)]) {
        let live_handles: HashSet<&str> = windows
            .iter()
            .map(|(handle, _, _)| handle.as_str())
            .collect();
        let removed_handles: Vec<String> = state
            .handle_to_tab_id
            .keys()
            .filter(|handle| !live_handles.contains(handle.as_str()))
            .cloned()
            .collect();

        for handle in removed_handles {
            if let Some(tab_id) = state.handle_to_tab_id.remove(&handle) {
                state.tabs.remove(&tab_id);
                if state.active_tab_id.as_ref() == Some(&tab_id) {
                    state.active_tab_id = None;
                }
            }
        }

        for (handle, url, title) in windows {
            self.ensure_tab_for_window(state, handle.clone(), url.clone(), title.clone());
        }
    }

    fn remove_tab(&self, state: &mut TabState, tab_id: &str) {
        if let Some(removed) = state.tabs.remove(tab_id) {
            state.handle_to_tab_id.remove(&removed.window_handle);
        }
        if state.active_tab_id.as_deref() == Some(tab_id) {
            state.active_tab_id = None;
        }
    }

    fn allocate_tab_id(&self) -> String {
        let next = self.next_tab_counter.fetch_add(1, Ordering::Relaxed) + 1;
        format!("tab-{next}")
    }
}

pub(crate) struct ChromeDriverProcess {
    child: Mutex<Option<Child>>,
    port: u16,
    driver_binary: PathBuf,
}

impl fmt::Debug for ChromeDriverProcess {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChromeDriverProcess")
            .field("port", &self.port)
            .field("driver_binary", &self.driver_binary)
            .finish_non_exhaustive()
    }
}

impl ChromeDriverProcess {
    #[must_use]
    pub(crate) fn new(child: Child, port: u16, driver_binary: PathBuf) -> Self {
        Self {
            child: Mutex::new(Some(child)),
            port,
            driver_binary,
        }
    }

    pub(crate) fn server_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub(crate) async fn is_alive(&self) -> bool {
        let mut guard = self.child.lock().await;
        let is_alive = match guard.as_mut() {
            None => false,
            Some(child) => child
                .try_wait()
                .map(|status| status.is_none())
                .unwrap_or(false),
        };
        if !is_alive {
            guard.take();
        }
        is_alive
    }

    pub(crate) fn matches_driver_binary(&self, driver_binary: &Path) -> bool {
        self.driver_binary == driver_binary
    }

    pub(crate) async fn shutdown(&self) -> Result<(), ChromeToolError> {
        let mut child = self.child.lock().await.take();
        if let Some(child) = child.as_mut() {
            shutdown_child_process(child).await?;
        }
        Ok(())
    }
}

pub struct ManagedWebDriverSession {
    driver: Mutex<Option<WebDriver>>,
}

impl fmt::Debug for ManagedWebDriverSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ManagedWebDriverSession")
            .finish_non_exhaustive()
    }
}

impl ManagedWebDriverSession {
    #[must_use]
    pub fn new(driver: WebDriver) -> Self {
        Self {
            driver: Mutex::new(Some(driver)),
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

    async fn click(&self, selector: &str) -> Result<(), ChromeToolError> {
        let driver = self.live_driver().await?;
        let element = driver.find(By::Css(selector)).await.map_err(|e| {
            ChromeToolError::InteractionFailed {
                reason: format!("element not found for selector '{selector}': {e}"),
            }
        })?;
        element
            .click()
            .await
            .map_err(|e| ChromeToolError::InteractionFailed {
                reason: format!("click failed for selector '{selector}': {e}"),
            })
    }

    async fn type_text(&self, selector: &str, text: &str) -> Result<(), ChromeToolError> {
        let driver = self.live_driver().await?;
        let element = driver.find(By::Css(selector)).await.map_err(|e| {
            ChromeToolError::InteractionFailed {
                reason: format!("element not found for selector '{selector}': {e}"),
            }
        })?;
        element
            .send_keys(text)
            .await
            .map_err(|e| ChromeToolError::InteractionFailed {
                reason: format!("type failed for selector '{selector}': {e}"),
            })
    }

    async fn current_url(&self) -> Result<String, ChromeToolError> {
        let driver = self.live_driver().await?;
        driver
            .current_url()
            .await
            .map(|url| url.to_string())
            .map_err(|e| ChromeToolError::PageReadFailed {
                reason: format!("failed to get current URL: {e}"),
            })
    }

    async fn get_cookies(&self) -> Result<Vec<Cookie>, ChromeToolError> {
        let driver = self.live_driver().await?;
        driver
            .get_all_cookies()
            .await
            .map_err(|e| ChromeToolError::PageReadFailed {
                reason: format!("failed to get cookies: {e}"),
            })
    }

    async fn navigate(&self, url: &str) -> Result<PageMetadata, ChromeToolError> {
        let driver = self.live_driver().await?;
        driver
            .goto(url)
            .await
            .map_err(|e| ChromeToolError::NavigationFailed {
                url: url.to_string(),
                reason: e.to_string(),
            })?;
        let final_url = driver
            .current_url()
            .await
            .map(|u| u.to_string())
            .map_err(|e| ChromeToolError::PageReadFailed {
                reason: e.to_string(),
            })?;
        let page_title = driver
            .title()
            .await
            .map_err(|e| ChromeToolError::PageReadFailed {
                reason: e.to_string(),
            })?;
        Ok(PageMetadata {
            final_url,
            page_title,
        })
    }

    async fn create_new_tab(&self, url: &str) -> Result<(String, PageMetadata), ChromeToolError> {
        let driver = self.live_driver().await?;
        let original_handle = driver.window().await.ok();
        let handle = driver
            .new_tab()
            .await
            .map_err(|e| ChromeToolError::TabOperationFailed {
                reason: format!("failed to create new tab: {e}"),
            })?;
        let window_handle = handle.to_string();
        driver
            .switch_to_window(handle)
            .await
            .map_err(|e| ChromeToolError::TabOperationFailed {
                reason: format!("failed to switch to new tab: {e}"),
            })?;
        if let Err(e) = driver.goto(url).await {
            // Roll back: close the empty new tab and switch back to original
            let _ = driver.close_window().await;
            if let Some(original) = original_handle {
                let _ = driver.switch_to_window(original).await;
            }
            return Err(ChromeToolError::NavigationFailed {
                url: url.to_string(),
                reason: e.to_string(),
            });
        }
        let final_url = driver
            .current_url()
            .await
            .map(|u| u.to_string())
            .map_err(|e| ChromeToolError::PageReadFailed {
                reason: e.to_string(),
            })?;
        let page_title = driver
            .title()
            .await
            .map_err(|e| ChromeToolError::PageReadFailed {
                reason: e.to_string(),
            })?;
        Ok((
            window_handle,
            PageMetadata {
                final_url,
                page_title,
            },
        ))
    }

    async fn switch_to_window(&self, window_handle: &str) -> Result<PageMetadata, ChromeToolError> {
        let driver = self.live_driver().await?;
        let handle = thirtyfour::WindowHandle::from(window_handle.to_string());
        driver
            .switch_to_window(handle)
            .await
            .map_err(|e| ChromeToolError::TabOperationFailed {
                reason: format!("failed to switch to window '{window_handle}': {e}"),
            })?;
        let final_url = driver
            .current_url()
            .await
            .map(|u| u.to_string())
            .map_err(|e| ChromeToolError::PageReadFailed {
                reason: e.to_string(),
            })?;
        let page_title = driver
            .title()
            .await
            .map_err(|e| ChromeToolError::PageReadFailed {
                reason: e.to_string(),
            })?;
        Ok(PageMetadata {
            final_url,
            page_title,
        })
    }

    async fn close_current_window(&self) -> Result<(), ChromeToolError> {
        let driver = self.live_driver().await?;
        driver
            .close_window()
            .await
            .map_err(|e| ChromeToolError::TabOperationFailed {
                reason: format!("failed to close window: {e}"),
            })
    }

    async fn list_windows(&self) -> Result<Vec<(String, String, String)>, ChromeToolError> {
        let driver = self.live_driver().await?;
        let current_handle =
            driver
                .window()
                .await
                .map_err(|e| ChromeToolError::TabOperationFailed {
                    reason: format!("failed to get current window: {e}"),
                })?;
        let handles = driver
            .windows()
            .await
            .map_err(|e| ChromeToolError::TabOperationFailed {
                reason: format!("failed to list windows: {e}"),
            })?;
        let mut result = Vec::with_capacity(handles.len());
        for handle in handles {
            let handle_str = handle.to_string();
            if handle != current_handle {
                let switch_result = driver.switch_to_window(handle.clone()).await;
                if switch_result.is_err() {
                    continue;
                }
            }
            let url = driver
                .current_url()
                .await
                .map(|u| u.to_string())
                .unwrap_or_default();
            let title = driver.title().await.unwrap_or_default();
            result.push((handle_str, url, title));
        }
        // Switch back to original window
        let _ = driver.switch_to_window(current_handle).await;
        Ok(result)
    }

    async fn current_window_handle(&self) -> Result<String, ChromeToolError> {
        let driver = self.live_driver().await?;
        driver.window().await.map(|h| h.to_string()).map_err(|e| {
            ChromeToolError::TabOperationFailed {
                reason: format!("failed to get current window handle: {e}"),
            }
        })
    }

    async fn shutdown(&self) -> Result<(), ChromeToolError> {
        if let Some(driver) = self.driver.lock().await.take() {
            driver
                .quit()
                .await
                .map_err(|e| ChromeToolError::SessionShutdownFailed {
                    reason: e.to_string(),
                })?;
        }
        Ok(())
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

#[cfg(test)]
mod tests {
    use serde_json::json;
    use thirtyfour::prelude::{DesiredCapabilities, WebDriver};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::{BrowserSession, ChromeToolError, ManagedWebDriverSession};

    async fn read_http_request(
        stream: &mut tokio::net::TcpStream,
    ) -> std::io::Result<(String, Vec<u8>)> {
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 1024];
        let mut header_end = None;
        let mut content_length = 0_usize;

        loop {
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);

            if header_end.is_none()
                && let Some(index) = buffer.windows(4).position(|window| window == b"\r\n\r\n")
            {
                header_end = Some(index + 4);
                let headers = String::from_utf8_lossy(&buffer[..index + 4]);
                for line in headers.lines() {
                    if let Some(value) = line.strip_prefix("Content-Length:") {
                        content_length = value.trim().parse().unwrap_or(0);
                    }
                }
            }

            if let Some(end) = header_end
                && buffer.len() >= end + content_length
            {
                let request_line = String::from_utf8_lossy(&buffer[..end])
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .to_string();
                return Ok((request_line, buffer));
            }
        }

        let request_line = String::from_utf8_lossy(&buffer)
            .lines()
            .next()
            .unwrap_or_default()
            .to_string();
        Ok((request_line, buffer))
    }

    async fn write_json_response(
        stream: &mut tokio::net::TcpStream,
        status: &str,
        body: serde_json::Value,
    ) -> std::io::Result<()> {
        let body = body.to_string();
        let response = format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).await?;
        stream.shutdown().await
    }

    #[tokio::test]
    async fn managed_webdriver_session_shutdown_propagates_quit_failures() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for expected_request in [
                "POST /session HTTP/1.1",
                "POST /session/test-session/timeouts HTTP/1.1",
                "DELETE /session/test-session HTTP/1.1",
            ] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let (request_line, _) = read_http_request(&mut stream).await.unwrap();
                assert_eq!(request_line, expected_request);

                let response = if expected_request.starts_with("DELETE ") {
                    (
                        "500 Internal Server Error",
                        json!({
                            "value": {
                                "error": "unknown error",
                                "message": "quit failed",
                                "stacktrace": ""
                            }
                        }),
                    )
                } else if expected_request.starts_with("POST /session ") {
                    (
                        "200 OK",
                        json!({
                            "value": {
                                "sessionId": "test-session",
                                "capabilities": {
                                    "browserName": "chrome"
                                }
                            }
                        }),
                    )
                } else {
                    ("200 OK", json!({ "value": null }))
                };

                write_json_response(&mut stream, response.0, response.1)
                    .await
                    .unwrap();
            }
        });

        let driver = WebDriver::new(
            format!("http://{server_addr}"),
            DesiredCapabilities::chrome(),
        )
        .await
        .unwrap();
        let session = ManagedWebDriverSession::new(driver);

        let error = session.shutdown().await.unwrap_err();
        assert!(matches!(
            error,
            ChromeToolError::SessionShutdownFailed { .. }
        ));

        server.await.unwrap();
    }
}
