use std::collections::HashMap;
use std::fmt;
use std::sync::Arc;

use async_trait::async_trait;
use serde::Deserialize;
use serde_json::{Value, json};
use thirtyfour::RequestData;
use thirtyfour::common::command::FormatRequestData;
use thirtyfour::prelude::{By, WebDriver};
use tokio::process::Child;
use tokio::sync::Mutex;

use super::error::ChromeToolError;
use super::models::{CookieSummary, LinkSummary, NetworkRequestSummary, PageMetadata};

#[async_trait]
pub trait BrowserSession: Send + Sync {
    async fn extract_text(&self, selector: Option<&str>) -> Result<String, ChromeToolError>;
    async fn list_links(&self) -> Result<Vec<LinkSummary>, ChromeToolError>;
    async fn network_requests(
        &self,
        max_requests: Option<u32>,
    ) -> Result<Vec<NetworkRequestSummary>, ChromeToolError>;
    async fn shutdown(&self) -> Result<(), ChromeToolError>;
    async fn click(&self, selector: &str) -> Result<(), ChromeToolError>;
    async fn type_text(&self, selector: &str, text: &str) -> Result<(), ChromeToolError>;
    async fn current_url(&self) -> Result<String, ChromeToolError>;
    async fn get_cookies(&self) -> Result<Vec<CookieSummary>, ChromeToolError>;
    async fn navigate(&self, url: &str) -> Result<PageMetadata, ChromeToolError>;
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

    pub fn update_metadata(&mut self, metadata: PageMetadata) {
        self.current_url = metadata.final_url;
        self.page_title = metadata.page_title;
    }
}

pub(crate) struct ChromeDriverProcess {
    child: Mutex<Option<Child>>,
    port: u16,
}

impl fmt::Debug for ChromeDriverProcess {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChromeDriverProcess")
            .field("port", &self.port)
            .finish_non_exhaustive()
    }
}

impl ChromeDriverProcess {
    #[must_use]
    pub(crate) fn new(child: Child, port: u16) -> Self {
        Self {
            child: Mutex::new(Some(child)),
            port,
        }
    }

    pub(crate) fn server_url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    pub(crate) async fn is_alive(&self) -> bool {
        let mut guard = self.child.lock().await;
        match guard.as_mut() {
            None => false,
            Some(child) => child
                .try_wait()
                .map(|status| status.is_none())
                .unwrap_or(false),
        }
    }
}

pub struct ManagedWebDriverSession {
    driver: Mutex<Option<WebDriver>>,
    network_requests: Mutex<NetworkRequestTracker>,
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
            network_requests: Mutex::new(NetworkRequestTracker::default()),
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

#[derive(Debug)]
enum ChromeLogCommand {
    GetLog { log_type: &'static str },
}

impl FormatRequestData for ChromeLogCommand {
    fn format_request(&self, session_id: &thirtyfour::SessionId) -> RequestData {
        match self {
            Self::GetLog { log_type } => RequestData::new(
                "POST".parse().expect("POST is always a valid HTTP method"),
                format!("/session/{session_id}/se/log"),
            )
            .add_body(json!({ "type": log_type })),
        }
    }
}

#[derive(Debug, Default)]
struct NetworkRequestTracker {
    order: Vec<String>,
    requests: HashMap<String, NetworkRequestSummary>,
}

impl NetworkRequestTracker {
    fn request_mut(&mut self, request_id: &str) -> &mut NetworkRequestSummary {
        if !self.requests.contains_key(request_id) {
            self.order.push(request_id.to_string());
            self.requests.insert(
                request_id.to_string(),
                NetworkRequestSummary {
                    method: String::new(),
                    url: String::new(),
                    status: None,
                    request_headers: json!({}),
                    response_headers: json!({}),
                    error: None,
                },
            );
        }

        self.requests
            .get_mut(request_id)
            .expect("request must exist after insertion")
    }

    fn summaries(&self) -> Vec<NetworkRequestSummary> {
        self.order
            .iter()
            .filter_map(|request_id| self.requests.get(request_id))
            .filter(|request| !request.url.is_empty())
            .map(|request| {
                let mut request = request.clone();
                if request.method.is_empty() {
                    request.method = "UNKNOWN".to_string();
                }
                if !request.request_headers.is_object() {
                    request.request_headers = json!({});
                }
                if !request.response_headers.is_object() {
                    request.response_headers = json!({});
                }
                request
            })
            .collect()
    }
}

#[derive(Debug, Deserialize)]
struct WebDriverLogEntry {
    message: String,
}

#[derive(Debug, Deserialize)]
struct PerformanceLogEnvelope {
    message: PerformanceLogMessage,
}

#[derive(Debug, Deserialize)]
struct PerformanceLogMessage {
    method: String,
    params: Value,
}

fn apply_performance_log_entries(
    tracker: &mut NetworkRequestTracker,
    entries: Vec<WebDriverLogEntry>,
) {
    for entry in entries {
        let Ok(envelope) = serde_json::from_str::<PerformanceLogEnvelope>(&entry.message) else {
            continue;
        };

        match envelope.message.method.as_str() {
            "Network.requestWillBeSent" => {
                update_request_started(tracker, &envelope.message.params);
            }
            "Network.requestWillBeSentExtraInfo" => {
                update_request_extra_info(tracker, &envelope.message.params);
            }
            "Network.responseReceived" => {
                update_response_received(tracker, &envelope.message.params);
            }
            "Network.responseReceivedExtraInfo" => {
                update_response_extra_info(tracker, &envelope.message.params);
            }
            "Network.loadingFailed" => {
                update_request_failed(tracker, &envelope.message.params);
            }
            _ => {}
        }
    }
}

fn update_request_started(tracker: &mut NetworkRequestTracker, params: &Value) {
    let Some(request_id) = request_id(params) else {
        return;
    };
    let Some(request) = params.get("request") else {
        return;
    };

    let tracked = tracker.request_mut(request_id);
    if let Some(method) = request.get("method").and_then(Value::as_str) {
        tracked.method = method.to_string();
    }
    if let Some(url) = request.get("url").and_then(Value::as_str) {
        tracked.url = url.to_string();
    }
    tracked.request_headers = normalized_headers(request.get("headers"));
}

fn update_request_extra_info(tracker: &mut NetworkRequestTracker, params: &Value) {
    let Some(request_id) = request_id(params) else {
        return;
    };
    let tracked = tracker.request_mut(request_id);
    tracked.request_headers = normalized_headers(params.get("headers"));
}

fn update_response_received(tracker: &mut NetworkRequestTracker, params: &Value) {
    let Some(request_id) = request_id(params) else {
        return;
    };
    let Some(response) = params.get("response") else {
        return;
    };

    let tracked = tracker.request_mut(request_id);
    if let Some(url) = response.get("url").and_then(Value::as_str)
        && tracked.url.is_empty()
    {
        tracked.url = url.to_string();
    }
    tracked.status = response.get("status").and_then(parse_status_code);
    tracked.response_headers = normalized_headers(response.get("headers"));
}

fn update_response_extra_info(tracker: &mut NetworkRequestTracker, params: &Value) {
    let Some(request_id) = request_id(params) else {
        return;
    };
    let tracked = tracker.request_mut(request_id);
    if let Some(status) = params.get("statusCode").and_then(parse_status_code) {
        tracked.status = Some(status);
    }
    tracked.response_headers = normalized_headers(params.get("headers"));
}

fn update_request_failed(tracker: &mut NetworkRequestTracker, params: &Value) {
    let Some(request_id) = request_id(params) else {
        return;
    };
    let tracked = tracker.request_mut(request_id);
    if let Some(error) = params.get("errorText").and_then(Value::as_str) {
        tracked.error = Some(error.to_string());
    }
}

fn request_id(params: &Value) -> Option<&str> {
    params.get("requestId").and_then(Value::as_str)
}

fn normalized_headers(headers: Option<&Value>) -> Value {
    match headers {
        Some(Value::Object(_)) => headers.cloned().unwrap_or_else(|| json!({})),
        _ => json!({}),
    }
}

fn parse_status_code(value: &Value) -> Option<u16> {
    value
        .as_u64()
        .and_then(|status| u16::try_from(status).ok())
        .or_else(|| {
            value.as_f64().and_then(|status| {
                if status.is_finite() && status >= 0.0 && status <= u16::MAX as f64 {
                    Some(status as u16)
                } else {
                    None
                }
            })
        })
        .or_else(|| value.as_str().and_then(|status| status.parse::<u16>().ok()))
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

    async fn network_requests(
        &self,
        max_requests: Option<u32>,
    ) -> Result<Vec<NetworkRequestSummary>, ChromeToolError> {
        let driver = self.live_driver().await?;
        let entries: Vec<WebDriverLogEntry> = driver
            .handle
            .cmd(ChromeLogCommand::GetLog {
                log_type: "performance",
            })
            .await
            .map_err(|e| ChromeToolError::PageReadFailed {
                reason: format!("failed to collect chrome performance logs: {e}"),
            })?
            .value()
            .map_err(|e| ChromeToolError::PageReadFailed {
                reason: format!("failed to parse chrome performance logs: {e}"),
            })?;

        let mut tracker = self.network_requests.lock().await;
        apply_performance_log_entries(&mut tracker, entries);
        let mut requests = tracker.summaries();

        if let Some(max_requests) = max_requests {
            requests.truncate(max_requests as usize);
        }

        Ok(requests)
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

    async fn shutdown(&self) -> Result<(), ChromeToolError> {
        if let Some(driver) = self.driver.lock().await.take() {
            let _ = driver.quit().await;
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

    use super::{
        ChromeToolError, NetworkRequestTracker, WebDriverLogEntry, apply_performance_log_entries,
    };

    #[test]
    fn performance_log_parser_collects_real_network_event_fields() {
        let mut tracker = NetworkRequestTracker::default();

        apply_performance_log_entries(
            &mut tracker,
            vec![
                WebDriverLogEntry {
                    message: json!({
                        "message": {
                            "method": "Network.requestWillBeSent",
                            "params": {
                                "requestId": "req-1",
                                "request": {
                                    "method": "POST",
                                    "url": "https://api.example.com/items",
                                    "headers": {
                                        "content-type": "application/json"
                                    }
                                }
                            }
                        }
                    })
                    .to_string(),
                },
                WebDriverLogEntry {
                    message: json!({
                        "message": {
                            "method": "Network.responseReceived",
                            "params": {
                                "requestId": "req-1",
                                "response": {
                                    "url": "https://api.example.com/items",
                                    "status": 201.0,
                                    "headers": {
                                        "content-type": "application/json"
                                    }
                                }
                            }
                        }
                    })
                    .to_string(),
                },
                WebDriverLogEntry {
                    message: json!({
                        "message": {
                            "method": "Network.requestWillBeSent",
                            "params": {
                                "requestId": "req-2",
                                "request": {
                                    "method": "GET",
                                    "url": "https://cdn.example.com/app.js",
                                    "headers": {}
                                }
                            }
                        }
                    })
                    .to_string(),
                },
                WebDriverLogEntry {
                    message: json!({
                        "message": {
                            "method": "Network.loadingFailed",
                            "params": {
                                "requestId": "req-2",
                                "errorText": "net::ERR_ABORTED"
                            }
                        }
                    })
                    .to_string(),
                },
                WebDriverLogEntry {
                    message: json!({
                        "message": {
                            "method": "Page.loadEventFired",
                            "params": {}
                        }
                    })
                    .to_string(),
                },
            ],
        );

        let requests = tracker.summaries();
        assert_eq!(requests.len(), 2);
        assert_eq!(requests[0].method, "POST");
        assert_eq!(requests[0].url, "https://api.example.com/items");
        assert_eq!(requests[0].status, Some(201));
        assert_eq!(
            requests[0].request_headers,
            json!({ "content-type": "application/json" })
        );
        assert_eq!(
            requests[0].response_headers,
            json!({ "content-type": "application/json" })
        );
        assert_eq!(requests[0].error, None);

        assert_eq!(requests[1].method, "GET");
        assert_eq!(requests[1].url, "https://cdn.example.com/app.js");
        assert_eq!(requests[1].status, None);
        assert_eq!(requests[1].error.as_deref(), Some("net::ERR_ABORTED"));
    }
}
