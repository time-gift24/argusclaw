use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;
use thirtyfour::extensions::cdp::ChromeDevTools;
use thirtyfour::prelude::{By, WebDriver};
use tokio::process::Child;
use tokio::sync::Mutex;

use super::error::ChromeToolError;
use super::models::{CookieSummary, LinkSummary, NetworkRequestSummary};

const NETWORK_RECORDER_BOOTSTRAP_SCRIPT: &str = r#"
(() => {
    const stateKey = "__argusChromeNetworkRecorder";
    if (window[stateKey]) {
        return;
    }

    const maxEntries = 512;
    const state = { requests: [] };

    const trimRequests = () => {
        if (state.requests.length > maxEntries) {
            state.requests.splice(0, state.requests.length - maxEntries);
        }
    };

    const headersToObject = (value) => {
        const result = {};
        if (!value) {
            return result;
        }

        try {
            if (typeof Headers !== "undefined" && value instanceof Headers) {
                value.forEach((headerValue, headerName) => {
                    result[String(headerName).toLowerCase()] = String(headerValue);
                });
                return result;
            }

            if (Array.isArray(value)) {
                for (const entry of value) {
                    if (Array.isArray(entry) && entry.length >= 2) {
                        result[String(entry[0]).toLowerCase()] = String(entry[1]);
                    }
                }
                return result;
            }

            if (typeof value.entries === "function") {
                for (const [headerName, headerValue] of value.entries()) {
                    result[String(headerName).toLowerCase()] = String(headerValue);
                }
                return result;
            }

            if (typeof value === "object") {
                for (const [headerName, headerValue] of Object.entries(value)) {
                    if (headerValue != null) {
                        result[String(headerName).toLowerCase()] = Array.isArray(headerValue)
                            ? headerValue.map(String).join(", ")
                            : String(headerValue);
                    }
                }
            }
        } catch (_) {
            return result;
        }

        return result;
    };

    const parseRawHeaders = (rawHeaders) => {
        const result = {};
        if (!rawHeaders) {
            return result;
        }

        for (const line of String(rawHeaders).split(/\r?\n/)) {
            if (!line) {
                continue;
            }
            const separatorIndex = line.indexOf(":");
            if (separatorIndex <= 0) {
                continue;
            }
            const headerName = line.slice(0, separatorIndex).trim().toLowerCase();
            const headerValue = line.slice(separatorIndex + 1).trim();
            if (headerName) {
                result[headerName] = headerValue;
            }
        }
        return result;
    };

    const requestUrl = (input) => {
        if (typeof input === "string") {
            return input;
        }
        if (input && typeof input.url === "string") {
            return input.url;
        }
        return String(input || "");
    };

    const requestMethod = (input, init) => {
        if (init && typeof init.method === "string" && init.method) {
            return init.method.toUpperCase();
        }
        if (input && typeof input.method === "string" && input.method) {
            return input.method.toUpperCase();
        }
        return "GET";
    };

    const requestHeaders = (input, init) => {
        if (init && Object.prototype.hasOwnProperty.call(init, "headers")) {
            return headersToObject(init.headers);
        }
        if (input && input.headers) {
            return headersToObject(input.headers);
        }
        return {};
    };

    const record = (entry) => {
        state.requests.push(entry);
        trimRequests();
    };

    Object.defineProperty(window, stateKey, {
        value: state,
        configurable: false,
        enumerable: false,
        writable: false,
    });

    if (typeof window.fetch === "function") {
        const originalFetch = window.fetch.bind(window);
        window.fetch = async function(input, init) {
            const entry = {
                observed_at: performance.now(),
                method: requestMethod(input, init),
                url: requestUrl(input),
                status: null,
                request_headers: requestHeaders(input, init),
                response_headers: {},
                error: null,
            };

            try {
                const response = await originalFetch(input, init);
                entry.observed_at = performance.now();
                entry.status = Number.isFinite(response.status) ? response.status : null;
                entry.response_headers = headersToObject(response.headers);
                record(entry);
                return response;
            } catch (error) {
                entry.observed_at = performance.now();
                entry.error =
                    error && typeof error.message === "string" ? error.message : String(error);
                record(entry);
                throw error;
            }
        };
    }

    if (typeof XMLHttpRequest !== "undefined") {
        const originalOpen = XMLHttpRequest.prototype.open;
        const originalSetRequestHeader = XMLHttpRequest.prototype.setRequestHeader;
        const originalSend = XMLHttpRequest.prototype.send;

        XMLHttpRequest.prototype.open = function(method, url) {
            this.__argusRequestMeta = {
                method: String(method || "GET").toUpperCase(),
                url: String(url || ""),
                request_headers: {},
                observed_at: performance.now(),
            };
            return originalOpen.apply(this, arguments);
        };

        XMLHttpRequest.prototype.setRequestHeader = function(name, value) {
            const meta = this.__argusRequestMeta || (this.__argusRequestMeta = {
                method: "GET",
                url: "",
                request_headers: {},
                observed_at: performance.now(),
            });
            meta.request_headers[String(name).toLowerCase()] = String(value);
            return originalSetRequestHeader.apply(this, arguments);
        };

        XMLHttpRequest.prototype.send = function() {
            const xhr = this;
            const meta = xhr.__argusRequestMeta || {
                method: "GET",
                url: "",
                request_headers: {},
                observed_at: performance.now(),
            };

            const finalize = (errorMessage) => {
                record({
                    observed_at: performance.now(),
                    method: meta.method || "GET",
                    url: meta.url || "",
                    status: Number.isFinite(xhr.status) && xhr.status > 0 ? xhr.status : null,
                    request_headers: meta.request_headers || {},
                    response_headers: parseRawHeaders(
                        typeof xhr.getAllResponseHeaders === "function"
                            ? xhr.getAllResponseHeaders()
                            : ""
                    ),
                    error: errorMessage,
                });
                xhr.removeEventListener("loadend", onLoadEnd);
                xhr.removeEventListener("error", onError);
                xhr.removeEventListener("abort", onAbort);
                xhr.removeEventListener("timeout", onTimeout);
            };

            const onLoadEnd = () => finalize(null);
            const onError = () => finalize("network error");
            const onAbort = () => finalize("aborted");
            const onTimeout = () => finalize("timeout");

            xhr.addEventListener("loadend", onLoadEnd);
            xhr.addEventListener("error", onError);
            xhr.addEventListener("abort", onAbort);
            xhr.addEventListener("timeout", onTimeout);
            return originalSend.apply(xhr, arguments);
        };
    }
})();
"#;

const NETWORK_REQUEST_SNAPSHOT_SCRIPT: &str = r#"
return (function () {
    const state = window.__argusChromeNetworkRecorder;
    const runtimeRequests =
        state && Array.isArray(state.requests) ? state.requests.slice() : [];
    const performanceApi =
        window.performance && typeof window.performance.getEntriesByType === "function"
            ? window.performance
            : null;
    const navigationEntries = performanceApi
        ? performanceApi.getEntriesByType("navigation")
        : [];
    const navigationRequests = navigationEntries.map((entry) => ({
        observed_at: Number(entry.startTime || 0),
        method: "GET",
        url: String(entry.name || window.location.href || ""),
        status:
            typeof entry.responseStatus === "number" && entry.responseStatus > 0
                ? entry.responseStatus
                : null,
        request_headers: {},
        response_headers: {},
        error: null,
    }));
    const resourceEntries = performanceApi
        ? performanceApi.getEntriesByType("resource")
        : [];
    const resourceRequests = resourceEntries
        .filter((entry) => {
            const initiator = String(entry.initiatorType || "");
            return initiator !== "fetch" && initiator !== "xmlhttprequest";
        })
        .map((entry) => ({
            observed_at: Number(entry.startTime || 0),
            method: "GET",
            url: String(entry.name || ""),
            status:
                typeof entry.responseStatus === "number" && entry.responseStatus > 0
                    ? entry.responseStatus
                    : null,
            request_headers: {},
            response_headers: {},
            error: null,
        }));

    return [...navigationRequests, ...resourceRequests, ...runtimeRequests]
        .filter((entry) => typeof entry.url === "string" && entry.url.length > 0)
        .sort((left, right) => Number(left.observed_at || 0) - Number(right.observed_at || 0))
        .map((entry) => ({
            method: String(entry.method || "GET"),
            url: String(entry.url || ""),
            status: typeof entry.status === "number" ? entry.status : null,
            request_headers:
                entry.request_headers && typeof entry.request_headers === "object"
                    ? entry.request_headers
                    : {},
            response_headers:
                entry.response_headers && typeof entry.response_headers === "object"
                    ? entry.response_headers
                    : {},
            error: entry.error == null ? null : String(entry.error),
        }));
})();
"#;

pub(crate) async fn install_network_request_recorder(
    driver: &WebDriver,
) -> Result<(), ChromeToolError> {
    let dev_tools = ChromeDevTools::new(driver.handle.clone());
    dev_tools
        .execute_cdp_with_params(
            "Page.addScriptToEvaluateOnNewDocument",
            json!({ "source": NETWORK_RECORDER_BOOTSTRAP_SCRIPT }),
        )
        .await
        .map_err(|e| ChromeToolError::PageReadFailed {
            reason: format!("failed to install network request recorder: {e}"),
        })?;
    Ok(())
}

fn keep_recent_requests(
    mut requests: Vec<NetworkRequestSummary>,
    max_requests: Option<u32>,
) -> Vec<NetworkRequestSummary> {
    match max_requests {
        Some(limit) => requests.split_off(requests.len().saturating_sub(limit as usize)),
        None => requests,
    }
}

#[async_trait]
pub trait BrowserSession: Send + Sync {
    async fn extract_text(&self, selector: Option<&str>) -> Result<String, ChromeToolError>;
    async fn list_links(&self) -> Result<Vec<LinkSummary>, ChromeToolError>;
    async fn shutdown(&self) -> Result<(), ChromeToolError>;
    async fn click(&self, selector: &str) -> Result<(), ChromeToolError>;
    async fn type_text(&self, selector: &str, text: &str) -> Result<(), ChromeToolError>;
    async fn current_url(&self) -> Result<String, ChromeToolError>;
    async fn get_cookies(&self) -> Result<Vec<CookieSummary>, ChromeToolError>;
    async fn network_requests(
        &self,
        max_requests: Option<u32>,
    ) -> Result<Vec<NetworkRequestSummary>, ChromeToolError>;
}

#[derive(Clone)]
pub struct ChromeSession {
    pub session_id: String,
    pub current_url: String,
    pub page_title: String,
    interaction: Arc<dyn BrowserSession>,
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
        }
    }

    #[must_use]
    pub fn interaction(&self) -> Arc<dyn BrowserSession> {
        Arc::clone(&self.interaction)
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

    async fn get_cookies(&self) -> Result<Vec<CookieSummary>, ChromeToolError> {
        let driver = self.live_driver().await?;
        let cookies =
            driver
                .get_all_cookies()
                .await
                .map_err(|e| ChromeToolError::PageReadFailed {
                    reason: format!("failed to get cookies: {e}"),
                })?;
        Ok(cookies
            .into_iter()
            .map(|c| CookieSummary {
                name: c.name,
                value: c.value,
                domain: c.domain,
                path: c.path,
            })
            .collect())
    }

    async fn network_requests(
        &self,
        max_requests: Option<u32>,
    ) -> Result<Vec<NetworkRequestSummary>, ChromeToolError> {
        let driver = self.live_driver().await?;

        let entries = driver
            .execute(NETWORK_REQUEST_SNAPSHOT_SCRIPT.to_string(), vec![])
            .await
            .map_err(|e| ChromeToolError::PageReadFailed {
                reason: format!("failed to read network requests: {e}"),
            })?;

        let requests = entries
            .convert()
            .map_err(|e| ChromeToolError::PageReadFailed {
                reason: format!("failed to deserialize network requests: {e}"),
            })?;

        Ok(keep_recent_requests(requests, max_requests))
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

    use crate::chrome::models::NetworkRequestSummary;

    use super::{
        ChromeToolError, NETWORK_RECORDER_BOOTSTRAP_SCRIPT, NETWORK_REQUEST_SNAPSHOT_SCRIPT,
        finalize_shutdown, keep_recent_requests,
    };

    #[test]
    fn keep_recent_requests_prefers_latest_entries() {
        let requests = vec![
            NetworkRequestSummary {
                method: "GET".to_string(),
                url: "https://example.com/1".to_string(),
                status: Some(200),
                request_headers: serde_json::json!({}),
                response_headers: serde_json::json!({}),
                error: None,
            },
            NetworkRequestSummary {
                method: "GET".to_string(),
                url: "https://example.com/2".to_string(),
                status: Some(200),
                request_headers: serde_json::json!({}),
                response_headers: serde_json::json!({}),
                error: None,
            },
            NetworkRequestSummary {
                method: "GET".to_string(),
                url: "https://example.com/3".to_string(),
                status: Some(200),
                request_headers: serde_json::json!({}),
                response_headers: serde_json::json!({}),
                error: None,
            },
        ];

        let recent = keep_recent_requests(requests, Some(2));
        let urls: Vec<_> = recent.into_iter().map(|request| request.url).collect();
        assert_eq!(urls, vec!["https://example.com/2", "https://example.com/3"]);
    }

    #[test]
    fn network_scripts_cover_navigation_fetch_and_xhr() {
        assert!(NETWORK_REQUEST_SNAPSHOT_SCRIPT.contains("getEntriesByType(\"navigation\")"));
        assert!(NETWORK_REQUEST_SNAPSHOT_SCRIPT.contains("getEntriesByType(\"resource\")"));
        assert!(NETWORK_RECORDER_BOOTSTRAP_SCRIPT.contains("window.fetch"));
        assert!(NETWORK_RECORDER_BOOTSTRAP_SCRIPT.contains("XMLHttpRequest.prototype.open"));
    }

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
