//! Cookie manager: orchestrates Chrome connection and cookie storage.

use std::sync::Arc;

use tokio::sync::{RwLock, broadcast};
use tokio_util::sync::CancellationToken;

#[cfg(feature = "cookie")]
use super::chrome::ChromeConnection;
use super::store::CookieStore;
use super::types::{Cookie, CookieEvent};

/// Cookie manager: connects to Chrome and maintains cookie store.
pub struct CookieManager {
    #[cfg(feature = "cookie")]
    chrome: Option<Arc<ChromeConnection>>,
    store: Arc<RwLock<CookieStore>>,
    event_tx: broadcast::Sender<CookieEvent>,
    shutdown: CancellationToken,
}

impl CookieManager {
    /// Create empty manager (for testing or manual population).
    #[must_use]
    pub fn new() -> Self {
        let (event_tx, _) = broadcast::channel(256);
        Self {
            #[cfg(feature = "cookie")]
            chrome: None,
            store: Arc::new(RwLock::new(CookieStore::new())),
            event_tx,
            shutdown: CancellationToken::new(),
        }
    }

    /// Connect to Chrome and return connected manager.
    #[cfg(feature = "cookie")]
    pub async fn connect(port: u16) -> Result<Self, super::error::CookieError> {
        let chrome = ChromeConnection::connect(port).await?;
        let (event_tx, _) = broadcast::channel(256);
        Ok(Self {
            chrome: Some(Arc::new(chrome)),
            store: Arc::new(RwLock::new(CookieStore::new())),
            event_tx,
            shutdown: CancellationToken::new(),
        })
    }

    /// Check if connected to Chrome.
    #[cfg(feature = "cookie")]
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.chrome.is_some()
    }

    /// Get cookies for domain.
    pub async fn get_cookies(&self, domain: &str) -> Vec<Cookie> {
        self.store.read().await.get_by_domain(domain)
    }

    /// Get all cookies.
    pub async fn get_all_cookies(&self) -> Vec<Cookie> {
        self.store.read().await.get_all()
    }

    /// Subscribe to cookie events.
    pub fn subscribe(&self) -> broadcast::Receiver<CookieEvent> {
        self.event_tx.subscribe()
    }

    /// Add cookie manually (testing/population).
    pub async fn add_cookie(&self, cookie: Cookie) {
        let is_update = self.store.write().await.insert(cookie.clone());
        let event = if is_update {
            CookieEvent::Updated(cookie)
        } else {
            CookieEvent::Added(cookie)
        };
        let _ = self.event_tx.send(event);
    }

    /// Trigger graceful shutdown.
    pub fn shutdown(&self) {
        self.shutdown.cancel();
    }

    /// Check if shutdown triggered.
    #[must_use]
    pub fn is_shutdown(&self) -> bool {
        self.shutdown.is_cancelled()
    }
}

impl Default for CookieManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn new_manager_is_empty() {
        let manager = CookieManager::new();
        assert!(manager.get_all_cookies().await.is_empty());
    }

    #[tokio::test]
    async fn add_and_get_cookies() {
        let manager = CookieManager::new();

        let cookie = Cookie {
            name: "session".into(),
            value: "abc123".into(),
            domain: "example.com".into(),
            path: "/".into(),
            secure: false,
            http_only: false,
            same_site: None,
            expires: None,
        };

        manager.add_cookie(cookie).await;

        let cookies = manager.get_cookies("example.com").await;
        assert_eq!(cookies.len(), 1);
        assert_eq!(cookies[0].name, "session");
    }

    #[tokio::test]
    async fn subscribe_receives_events() {
        let manager = CookieManager::new();
        let mut rx = manager.subscribe();

        let cookie = Cookie {
            name: "test".into(),
            value: "val".into(),
            domain: "example.com".into(),
            path: "/".into(),
            secure: false,
            http_only: false,
            same_site: None,
            expires: None,
        };

        manager.add_cookie(cookie).await;

        let event = rx.try_recv();
        assert!(event.is_ok());
        assert!(matches!(event.unwrap(), CookieEvent::Added(c) if c.name == "test"));
    }

    #[tokio::test]
    async fn update_sends_update_event() {
        let manager = CookieManager::new();

        let cookie = Cookie {
            name: "test".into(),
            value: "val".into(),
            domain: "example.com".into(),
            path: "/".into(),
            secure: false,
            http_only: false,
            same_site: None,
            expires: None,
        };

        // Add first
        manager.add_cookie(cookie.clone()).await;

        // Update with same key
        let updated = Cookie {
            value: "new_val".into(),
            ..cookie
        };
        let mut rx = manager.subscribe();
        manager.add_cookie(updated).await;

        let event = rx.try_recv();
        assert!(event.is_ok());
        assert!(matches!(event.unwrap(), CookieEvent::Updated(c) if c.value == "new_val"));
    }

    #[tokio::test]
    async fn shutdown_works() {
        let manager = CookieManager::new();
        assert!(!manager.is_shutdown());

        manager.shutdown();
        assert!(manager.is_shutdown());
    }
}
