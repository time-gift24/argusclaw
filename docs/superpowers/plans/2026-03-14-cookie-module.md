# Cookie Module Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add Chrome cookie monitoring capability to ArgusClaw via CDP with in-memory storage and LLM tool interface.

**Architecture:** Cookie module as `claw::cookie` with CookieManager orchestrating ChromeConnection (CDP), CookieStore (memory), and event broadcasting. Optional feature gate `cookie` enables the module.

**Tech Stack:** Rust, chromiumoxide (CDP client), tokio broadcast channel, tokio_util::CancellationToken

**Spec:** `docs/superpowers/specs/2026-03-14-cookie-module-design.md`

**MVP Scope Note:** This plan implements the cookie module with manual cookie population (`add_cookie()`). Automatic CDP event listening for `Network.responseReceived` is out of scope and will be added in a follow-up task. This allows incremental delivery while maintaining a stable foundation.

---

## File Structure

```
crates/claw/src/
├── cookie/
│   ├── mod.rs           # Module entry, public API exports
│   ├── error.rs         # CookieError type
│   ├── types.rs         # Cookie, CookieKey, CookieEvent
│   ├── store.rs         # CookieStore (in-memory, indexed by domain)
│   ├── chrome.rs        # ChromeConnection (CDP wrapper)
│   ├── manager.rs       # CookieManager (orchestrator)
│   └── tool.rs          # GetCookiesTool
├── lib.rs               # Add: pub mod cookie;
└── claw.rs              # Add: #[cfg(feature = "cookie")] field

crates/claw/
└── Cargo.toml           # Add: chromiumoxide dep, cookie feature
```

---

## Chunk 1: Types and Error Foundation

### Task 1: Add Dependencies

**Files:**
- Modify: `crates/claw/Cargo.toml`

- [ ] **Step 1: Add chromiumoxide dependency**

Add to `[dependencies]` section:

```toml
chromiumoxide = { version = "0.7", features = ["tokio-runtime"], optional = true }
```

Add to `[features]` section:

```toml
cookie = ["dep:chromiumoxide"]
```

Note: `tokio-util` is already present for CancellationToken.

- [ ] **Step 2: Verify dependencies compile**

Run: `cargo check -p claw --features cookie`
Expected: Compiles successfully (may take time to download chromiumoxide)

- [ ] **Step 3: Commit**

```bash
git add crates/claw/Cargo.toml
git commit -m "feat(cookie): add chromiumoxide dependency behind feature gate"
```

---

### Task 2: Create Cookie Types

**Files:**
- Create: `crates/claw/src/cookie/mod.rs`
- Create: `crates/claw/src/cookie/types.rs`

- [ ] **Step 1: Create module directory**

```bash
mkdir -p crates/claw/src/cookie
```

- [ ] **Step 2: Write types.rs with Cookie, CookieKey, CookieEvent**

```rust
//! Cookie types: Cookie, CookieKey, CookieEvent

use chrono::{DateTime, Utc};

/// Single Cookie entry.
#[derive(Clone, Debug, PartialEq)]
pub struct Cookie {
    pub name: String,
    pub value: String,
    pub domain: String,
    pub path: String,
    pub secure: bool,
    pub http_only: bool,
    pub same_site: Option<String>,
    pub expires: Option<DateTime<Utc>>,
}

impl Cookie {
    /// Create a cookie with minimal fields (for testing).
    #[cfg(test)]
    pub fn new(name: impl Into<String>, domain: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            value: String::new(),
            domain: domain.into(),
            path: "/".to_string(),
            secure: false,
            http_only: false,
            same_site: None,
            expires: None,
        }
    }
}

/// Unique identifier for a cookie (name + domain + path).
#[derive(Clone, Debug, Hash, Eq, PartialEq)]
pub struct CookieKey {
    pub name: String,
    pub domain: String,
    pub path: String,
}

impl CookieKey {
    /// Create a key from a cookie reference.
    pub fn from_cookie(cookie: &Cookie) -> Self {
        Self {
            name: cookie.name.clone(),
            domain: cookie.domain.clone(),
            path: cookie.path.clone(),
        }
    }
}

/// Cookie change event for broadcast subscribers.
#[derive(Clone, Debug)]
pub enum CookieEvent {
    Added(Cookie),
    Updated(Cookie),
    Removed { domain: String, name: String },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cookie_key_from_cookie() {
        let cookie = Cookie {
            name: "session".into(),
            value: "abc123".into(),
            domain: "example.com".into(),
            path: "/app".into(),
            secure: true,
            http_only: false,
            same_site: Some("Lax".into()),
            expires: None,
        };

        let key = CookieKey::from_cookie(&cookie);

        assert_eq!(key.name, "session");
        assert_eq!(key.domain, "example.com");
        assert_eq!(key.path, "/app");
    }

    #[test]
    fn cookie_key_equality() {
        let k1 = CookieKey {
            name: "a".into(),
            domain: "ex.com".into(),
            path: "/".into(),
        };
        let k2 = CookieKey {
            name: "a".into(),
            domain: "ex.com".into(),
            path: "/".into(),
        };
        let k3 = CookieKey {
            name: "b".into(),
            domain: "ex.com".into(),
            path: "/".into(),
        };

        assert_eq!(k1, k2);
        assert_ne!(k1, k3);
    }
}
```

- [ ] **Step 3: Create mod.rs with exports**

```rust
//! Cookie management module for Chrome browser integration.
//!
//! Provides real-time cookie monitoring via Chrome DevTools Protocol (CDP).

mod error;
mod manager;
mod store;
mod types;

#[cfg(feature = "cookie")]
mod chrome;
#[cfg(feature = "cookie")]
mod tool;

pub use error::CookieError;
pub use manager::CookieManager;
pub use store::CookieStore;
pub use types::{Cookie, CookieEvent, CookieKey};

#[cfg(feature = "cookie")]
pub use chrome::ChromeConnection;
#[cfg(feature = "cookie")]
pub use tool::GetCookiesTool;
```

- [ ] **Step 4: Run tests to verify**

Run: `cargo test -p claw --features cookie --lib cookie::types`
Expected: 2 tests pass

- [ ] **Step 5: Commit**

```bash
git add crates/claw/src/cookie/
git commit -m "feat(cookie): add Cookie, CookieKey, CookieEvent types"
```

---

### Task 3: Create CookieError

**Files:**
- Create: `crates/claw/src/cookie/error.rs`

- [ ] **Step 1: Write error.rs**

```rust
//! Cookie module error types.

use thiserror::Error;

/// Errors from cookie operations.
#[derive(Debug, Error)]
pub enum CookieError {
    #[error("Failed to connect to Chrome: {reason}")]
    ConnectionFailed { reason: String },

    #[error("Chrome not running with remote debugging port")]
    DebuggingPortNotEnabled,

    #[error("CDP error: {0}")]
    #[cfg(feature = "cookie")]
    CdpError(#[from] chromiumoxide::error::CdpError),

    #[error("Invalid cookie format: {raw}")]
    InvalidCookieFormat { raw: String },

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check -p claw --features cookie`
Expected: Compiles successfully

- [ ] **Step 3: Commit**

```bash
git add crates/claw/src/cookie/error.rs
git commit -m "feat(cookie): add CookieError type"
```

---

## Chunk 2: CookieStore Implementation

### Task 4: Implement CookieStore (TDD)

**Files:**
- Create: `crates/claw/src/cookie/store.rs`

- [ ] **Step 1: Write store.rs with stub implementation**

```rust
//! In-memory cookie storage with domain indexing.

use std::collections::HashMap;

use super::types::{Cookie, CookieKey};

/// In-memory cookie store indexed by domain.
#[derive(Debug, Default)]
pub struct CookieStore {
    by_domain: HashMap<String, Vec<Cookie>>,
    index: HashMap<CookieKey, Cookie>,
}

impl CookieStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, _cookie: Cookie) -> bool {
        todo!()
    }

    pub fn remove(&mut self, _key: &CookieKey) -> Option<Cookie> {
        todo!()
    }

    pub fn get_by_domain(&self, _domain: &str) -> Vec<Cookie> {
        todo!()
    }

    pub fn get_all(&self) -> Vec<Cookie> {
        todo!()
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }
}
```

- [ ] **Step 2: Write failing tests**

Add to store.rs after the impl block:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn test_cookie(name: &str, domain: &str) -> Cookie {
        Cookie {
            name: name.to_string(),
            value: "value".to_string(),
            domain: domain.to_string(),
            path: "/".to_string(),
            secure: false,
            http_only: false,
            same_site: None,
            expires: None,
        }
    }

    #[test]
    fn new_store_is_empty() {
        let store = CookieStore::new();
        assert!(store.is_empty());
        assert_eq!(store.len(), 0);
    }

    #[test]
    fn insert_and_get_by_domain() {
        let mut store = CookieStore::new();
        store.insert(test_cookie("a", "example.com"));
        store.insert(test_cookie("b", "example.com"));
        store.insert(test_cookie("c", "other.com"));

        assert_eq!(store.get_by_domain("example.com").len(), 2);
        assert_eq!(store.get_by_domain("other.com").len(), 1);
        assert!(store.get_by_domain("nonexistent.com").is_empty());
    }

    #[test]
    fn insert_returns_false_for_new_cookie() {
        let mut store = CookieStore::new();
        assert!(!store.insert(test_cookie("a", "example.com")));
    }

    #[test]
    fn insert_returns_true_for_update() {
        let mut store = CookieStore::new();
        store.insert(test_cookie("a", "example.com"));

        let updated = Cookie {
            value: "new_value".to_string(),
            ..test_cookie("a", "example.com")
        };
        assert!(store.insert(updated));
        assert_eq!(store.get_by_domain("example.com")[0].value, "new_value");
    }

    #[test]
    fn remove_existing_cookie() {
        let mut store = CookieStore::new();
        store.insert(test_cookie("a", "example.com"));

        let key = CookieKey::from_cookie(&test_cookie("a", "example.com"));
        assert!(store.remove(&key).is_some());
        assert!(store.is_empty());
    }

    #[test]
    fn remove_nonexistent_returns_none() {
        let store = CookieStore::new();
        let key = CookieKey::from_cookie(&test_cookie("a", "example.com"));
        assert!(store.remove(&key).is_none());
    }

    #[test]
    fn get_all_returns_all_cookies() {
        let mut store = CookieStore::new();
        store.insert(test_cookie("a", "example.com"));
        store.insert(test_cookie("b", "example.com"));
        store.insert(test_cookie("c", "other.com"));

        assert_eq!(store.get_all().len(), 3);
    }
}
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test -p claw --features cookie --lib cookie::store 2>&1`
Expected: Tests fail with "not yet implemented" or panic

- [ ] **Step 4: Implement CookieStore to pass tests**

Replace the impl block with full implementation:

```rust
impl CookieStore {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, cookie: Cookie) -> bool {
        let key = CookieKey::from_cookie(&cookie);
        let is_update = self.index.contains_key(&key);

        self.index.insert(key.clone(), cookie.clone());

        let domain_cookies = self.by_domain.entry(cookie.domain.clone()).or_default();
        if is_update {
            if let Some(existing) = domain_cookies.iter_mut().find(|c| {
                c.name == cookie.name && c.path == cookie.path
            }) {
                *existing = cookie;
            }
        } else {
            domain_cookies.push(cookie);
        }

        is_update
    }

    pub fn remove(&mut self, key: &CookieKey) -> Option<Cookie> {
        let removed = self.index.remove(key)?;

        if let Some(domain_cookies) = self.by_domain.get_mut(&key.domain) {
            domain_cookies.retain(|c| !(c.name == key.name && c.path == key.path));
            if domain_cookies.is_empty() {
                self.by_domain.remove(&key.domain);
            }
        }

        Some(removed)
    }

    pub fn get_by_domain(&self, domain: &str) -> Vec<Cookie> {
        self.by_domain.get(domain).cloned().unwrap_or_default()
    }

    pub fn get_all(&self) -> Vec<Cookie> {
        self.index.values().cloned().collect()
    }

    pub fn len(&self) -> usize {
        self.index.len()
    }

    pub fn is_empty(&self) -> bool {
        self.index.is_empty()
    }
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test -p claw --features cookie --lib cookie::store`
Expected: 8 tests pass

- [ ] **Step 6: Commit**

```bash
git add crates/claw/src/cookie/store.rs
git commit -m "feat(cookie): add CookieStore with domain indexing"
```

---

## Chunk 3: Chrome CDP Connection

### Task 5: Implement ChromeConnection

**Files:**
- Create: `crates/claw/src/cookie/chrome.rs`

- [ ] **Step 1: Write chrome.rs**

```rust
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
    ///
    /// Returns `CookieError::ConnectionFailed` if Chrome is not running
    /// with `--remote-debugging-port`.
    pub async fn connect(port: u16) -> Result<Self, CookieError> {
        let ws_url = format!("ws://127.0.0.1:{}/devtools/browser", port);

        let browser = Browser::connect(&ws_url)
            .await
            .map_err(|e| CookieError::ConnectionFailed {
                reason: e.to_string(),
            })?;

        Ok(Self { browser })
    }

    /// Get a reference to the browser for event listening.
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
    #[ignore = "requires Chrome running with --remote-debugging-port=9222"]
    async fn connect_fails_on_wrong_port() {
        let result = ChromeConnection::connect(9999).await;
        assert!(result.is_err());
    }
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check -p claw --features cookie`
Expected: Compiles successfully

- [ ] **Step 3: Commit**

```bash
git add crates/claw/src/cookie/chrome.rs
git commit -m "feat(cookie): add ChromeConnection for CDP"
```

---

## Chunk 4: CookieManager Implementation

### Task 6: Implement CookieManager

**Files:**
- Create: `crates/claw/src/cookie/manager.rs`

- [ ] **Step 1: Write manager.rs**

```rust
//! Cookie manager: orchestrates Chrome connection and cookie storage.

use std::sync::Arc;

use tokio::sync::{RwLock, broadcast};
use tokio_util::sync::CancellationToken;

#[cfg(feature = "cookie")]
use super::chrome::ChromeConnection;
use super::error::CookieError;
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
    /// Create an empty manager (for testing or manual population).
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

    /// Connect to Chrome and return a connected manager.
    ///
    /// # Errors
    ///
    /// Returns `CookieError` if connection to Chrome fails.
    #[cfg(feature = "cookie")]
    pub async fn connect(port: u16) -> Result<Self, CookieError> {
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

    /// Get cookies for a specific domain.
    pub async fn get_cookies(&self, domain: &str) -> Vec<Cookie> {
        self.store.read().await.get_by_domain(domain)
    }

    /// Get all cookies.
    pub async fn get_all_cookies(&self) -> Vec<Cookie> {
        self.store.read().await.get_all()
    }

    /// Subscribe to cookie change events.
    pub fn subscribe(&self) -> broadcast::Receiver<CookieEvent> {
        self.event_tx.subscribe()
    }

    /// Manually add a cookie (for testing or manual population).
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

    /// Check if shutdown has been triggered.
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
            value: "v1".into(),
            domain: "example.com".into(),
            path: "/".into(),
            secure: false,
            http_only: false,
            same_site: None,
            expires: None,
        };

        manager.add_cookie(cookie).await;

        let mut rx = manager.subscribe();
        let updated = Cookie {
            value: "v2".into(),
            ..Cookie {
                name: "test".into(),
                domain: "example.com".into(),
                path: "/".into(),
                secure: false,
                http_only: false,
                same_site: None,
                expires: None,
            }
        };
        manager.add_cookie(updated).await;

        let event = rx.try_recv();
        assert!(matches!(event.ok(), Some(CookieEvent::Updated(_))));
    }

    #[tokio::test]
    async fn shutdown_cancels_token() {
        let manager = CookieManager::new();
        assert!(!manager.is_shutdown());

        manager.shutdown();

        assert!(manager.is_shutdown());
    }

    #[cfg(feature = "cookie")]
    #[tokio::test]
    async fn is_connected_false_when_not_connected() {
        let manager = CookieManager::new();
        assert!(!manager.is_connected());
    }

    #[cfg(feature = "cookie")]
    #[tokio::test]
    #[ignore = "requires Chrome running with --remote-debugging-port=9222"]
    async fn connect_to_chrome() {
        let result = CookieManager::connect(9222).await;
        assert!(result.is_ok());
        assert!(result.unwrap().is_connected());
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p claw --features cookie --lib cookie::manager`
Expected: 6 tests pass (2 ignored)

- [ ] **Step 3: Commit**

```bash
git add crates/claw/src/cookie/manager.rs
git commit -m "feat(cookie): add CookieManager with event broadcasting"
```

---

## Chunk 5: Tool Integration

### Task 7: Implement GetCookiesTool

**Files:**
- Create: `crates/claw/src/cookie/tool.rs`

- [ ] **Step 1: Write tool.rs**

```rust
//! Cookie tools for LLM/agent use.

use std::sync::Arc;

use async_trait::async_trait;
use serde_json::json;

use crate::llm::ToolDefinition;
use crate::protocol::RiskLevel;
use crate::tool::{NamedTool, ToolError};

use super::manager::CookieManager;

/// Tool to retrieve cookies for a domain.
pub struct GetCookiesTool {
    manager: Arc<CookieManager>,
}

impl GetCookiesTool {
    /// Create a new GetCookiesTool.
    #[must_use]
    pub fn new(manager: Arc<CookieManager>) -> Self {
        Self { manager }
    }
}

#[async_trait]
impl NamedTool for GetCookiesTool {
    fn name(&self) -> &str {
        "get_cookies"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "get_cookies".to_string(),
            description: "获取指定域名的 Cookie".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "domain": {
                        "type": "string",
                        "description": "目标域名，如 example.com"
                    }
                },
                "required": ["domain"]
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Low
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let domain = args["domain"].as_str().ok_or_else(|| {
            ToolError::ExecutionFailed {
                tool_name: "get_cookies".to_string(),
                reason: "Missing required parameter: domain".to_string(),
            }
        })?;

        let cookies = self.manager.get_cookies(domain).await;

        let cookie_header = cookies
            .iter()
            .map(|c| format!("{}={}", c.name, c.value))
            .collect::<Vec<_>>()
            .join("; ");

        Ok(json!({
            "cookies": cookies,
            "cookie_header": cookie_header
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cookie::Cookie;

    fn create_manager_with_cookie() -> Arc<CookieManager> {
        let manager = Arc::new(CookieManager::new());
        tokio::runtime::Runtime::new().unwrap().block_on(async {
            manager
                .add_cookie(Cookie {
                    name: "session".into(),
                    value: "abc123".into(),
                    domain: "example.com".into(),
                    path: "/".into(),
                    secure: false,
                    http_only: false,
                    same_site: None,
                    expires: None,
                })
                .await;
        });
        manager
    }

    #[test]
    fn tool_metadata() {
        let manager = Arc::new(CookieManager::new());
        let tool = GetCookiesTool::new(manager);

        assert_eq!(tool.name(), "get_cookies");
        assert_eq!(tool.risk_level(), RiskLevel::Low);
        assert!(tool.definition().description.contains("Cookie"));
    }

    #[tokio::test]
    async fn get_cookies_returns_empty_for_unknown_domain() {
        let manager = Arc::new(CookieManager::new());
        let tool = GetCookiesTool::new(manager);

        let result = tool
            .execute(json!({"domain": "unknown.com"}))
            .await
            .unwrap();

        assert_eq!(result["cookies"], json!([]));
        assert_eq!(result["cookie_header"], "");
    }

    #[tokio::test]
    async fn get_cookies_returns_cookies() {
        let manager = create_manager_with_cookie();
        let tool = GetCookiesTool::new(manager);

        let result = tool
            .execute(json!({"domain": "example.com"}))
            .await
            .unwrap();

        assert_eq!(result["cookies"].as_array().unwrap().len(), 1);
        assert_eq!(result["cookie_header"], "session=abc123");
    }

    #[tokio::test]
    async fn get_cookies_error_missing_domain() {
        let manager = Arc::new(CookieManager::new());
        let tool = GetCookiesTool::new(manager);

        let result = tool.execute(json!({})).await;

        assert!(result.is_err());
        match result {
            Err(ToolError::ExecutionFailed { tool_name, reason }) => {
                assert_eq!(tool_name, "get_cookies");
                assert!(reason.contains("domain"));
            }
            _ => panic!("Expected ExecutionFailed error"),
        }
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test -p claw --features cookie --lib cookie::tool`
Expected: 4 tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/claw/src/cookie/tool.rs
git commit -m "feat(cookie): add GetCookiesTool for LLM integration"
```

---

## Chunk 6: Module Export

### Task 8: Export cookie module from lib.rs

**Files:**
- Modify: `crates/claw/src/lib.rs`

- [ ] **Step 1: Add cookie module to lib.rs**

Add after line 11 (`pub mod tool;`):

```rust
#[cfg(feature = "cookie")]
pub mod cookie;
```

- [ ] **Step 2: Add public re-exports**

Add after line 17 (`pub use tool::{...};`):

```rust
#[cfg(feature = "cookie")]
pub use cookie::{Cookie, CookieError, CookieEvent, CookieManager, CookieStore};
#[cfg(feature = "cookie")]
pub use cookie::GetCookiesTool;
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check -p claw --features cookie`
Expected: Compiles successfully

- [ ] **Step 4: Commit**

```bash
git add crates/claw/src/lib.rs
git commit -m "feat(cookie): export cookie module from lib.rs"
```

---

## Chunk 7: AppContext Integration

### Task 9: Add CookieManager to AppContext

**Files:**
- Modify: `crates/claw/src/claw.rs`

- [ ] **Step 1: Add cookie_manager field to AppContext**

Add after line 28 (`shutdown: CancellationToken,`):

```rust
#[cfg(feature = "cookie")]
cookie_manager: Option<Arc<CookieManager>>,
```

- [ ] **Step 2: Update AppContext::init to initialize cookie_manager**

In the `init` method, add after the `shutdown` assignment:

```rust
#[cfg(feature = "cookie")]
let cookie_manager: Option<Arc<CookieManager>> = None;
```

Then update the `Ok(Self { ... })` block to include:

```rust
#[cfg(feature = "cookie")]
cookie_manager,
```

- [ ] **Step 3: Update AppContext::new and with_pool**

Add to both constructors:

```rust
#[cfg(feature = "cookie")]
cookie_manager: None,
```

- [ ] **Step 4: Add cookie manager accessor methods**

Add after the `shutdown()` method:

```rust
/// Initialize cookie manager (requires cookie feature and running Chrome).
#[cfg(feature = "cookie")]
pub async fn init_cookie_manager(&mut self, port: u16) -> Result<(), AgentError> {
    use crate::cookie::CookieError;

    let manager = CookieManager::connect(port)
        .await
        .map_err(|e| AgentError::CookieInitFailed {
            reason: e.to_string(),
        })?;
    self.cookie_manager = Some(Arc::new(manager));
    Ok(())
}

/// Get cookie manager.
#[cfg(feature = "cookie")]
#[must_use]
pub fn cookie_manager(&self) -> Option<&Arc<CookieManager>> {
    self.cookie_manager.as_ref()
}
```

- [ ] **Step 5: Add error variant to AgentError**

Open `crates/claw/src/error.rs` and add:

```rust
#[cfg(feature = "cookie")]
#[error("Failed to initialize cookie manager: {reason}")]
CookieInitFailed { reason: String },
```

- [ ] **Step 6: Add import at top of claw.rs**

Add after the existing imports:

```rust
#[cfg(feature = "cookie")]
use crate::cookie::CookieManager;
```

- [ ] **Step 7: Verify compilation**

Run: `cargo check -p claw --features cookie`
Expected: Compiles successfully

- [ ] **Step 8: Commit**

```bash
git add crates/claw/src/claw.rs crates/claw/src/error.rs
git commit -m "feat(cookie): integrate CookieManager into AppContext"
```

---

## Chunk 8: Final Verification

### Task 10: Final Verification

- [ ] **Step 1: Run all cookie tests**

Run: `cargo test -p claw --features cookie --lib cookie`
Expected: All tests pass

- [ ] **Step 2: Run full test suite without cookie feature**

Run: `cargo test -p claw`
Expected: All existing tests still pass

- [ ] **Step 3: Run clippy with cookie feature**

Run: `cargo clippy -p claw --features cookie`
Expected: No warnings

- [ ] **Step 4: Final commit (if any fixes)**

```bash
git add -A
git commit -m "fix(cookie): address clippy warnings"
```

---

## Manual Testing Checklist

After implementation, manually verify:

1. [ ] Chrome launches with `--remote-debugging-port=9222`
2. [ ] `CookieManager::connect(9222)` succeeds
3. [ ] `add_cookie()` and `get_cookies()` work correctly
4. [ ] `subscribe()` receives events when cookies are added
5. [ ] `GetCookiesTool` works via ToolManager
6. [ ] AppContext can initialize cookie manager

---

## Future Enhancements (Out of Scope for MVP)

- **CDP event listener**: Automatic cookie capture from `Network.responseReceived` and `Network.requestWillBeSent`
- **SetCookiesTool**: Inject cookies into browser
- **DeleteCookiesTool**: Remove cookies
- **Chrome reconnection**: Auto-reconnect on disconnect
- **Multi-tab isolation**: Per-tab cookie jars
- **Firefox support**: Via Marionette Protocol
