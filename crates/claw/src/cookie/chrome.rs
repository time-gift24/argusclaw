//! Chrome DevTools Protocol connection wrapper.

use chromiumoxide::Browser;

use super::error::CookieError;

/// CDP connection to Chrome browser.
pub struct ChromeConnection {
    browser: Browser,
}

impl ChromeConnection {
    /// Connect to Chrome via WebSocket.
    ///
    /// # Errors
    /// Returns `CookieError::ConnectionFailed` if the connection cannot be established.
    pub async fn connect(port: u16) -> Result<Self, CookieError> {
        let ws_url = format!("ws://127.0.0.1:{port}/devtools/browser");
        let (browser, _handler) =
            Browser::connect(&ws_url)
                .await
                .map_err(|e| CookieError::ConnectionFailed {
                    reason: e.to_string(),
                })?;
        Ok(Self { browser })
    }

    /// Get browser reference for advanced operations.
    pub fn browser(&self) -> &Browser {
        &self.browser
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires Chrome running with --remote-debugging-port=9222"]
    async fn connect_to_chrome() {
        let result = ChromeConnection::connect(9222).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[ignore = "requires no Chrome on port 9999"]
    async fn connect_fails_on_wrong_port() {
        let result = ChromeConnection::connect(9999).await;
        assert!(result.is_err());
    }
}
