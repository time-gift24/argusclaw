//! Host state and functions for WASM execution.
//!
//! This module provides the host-side state and functions that can be
//! called by WASM tools through the WIT interface.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;

use super::allowlist::SharedAllowlistValidator;
use super::capabilities::Capabilities;
use crate::tool::ToolManager;

/// Log level for WASM tool logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Debug,
    Info,
    Warn,
    Error,
}

impl std::fmt::Display for LogLevel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Debug => write!(f, "DEBUG"),
            Self::Info => write!(f, "INFO"),
            Self::Warn => write!(f, "WARN"),
            Self::Error => write!(f, "ERROR"),
        }
    }
}

/// A log entry from a WASM tool.
#[derive(Debug, Clone)]
pub struct LogEntry {
    /// Log level.
    pub level: LogLevel,
    /// Log message.
    pub message: String,
    /// Timestamp in milliseconds since execution started.
    pub timestamp_ms: u64,
}

/// Secrets store for WASM tools.
pub type SecretsStore = HashMap<String, String>;

/// Host state shared with WASM execution.
///
/// This struct contains all the state that host functions can access
/// during WASM execution.
#[derive(Debug)]
pub struct HostState {
    /// Tool capabilities (what the tool is allowed to do).
    pub capabilities: Arc<Capabilities>,

    /// HTTP allowlist validator.
    pub http_allowlist: SharedAllowlistValidator,

    /// Tool manager for invoking other tools.
    pub tool_manager: Option<Arc<ToolManager>>,

    /// Secrets store.
    pub secrets: Arc<RwLock<SecretsStore>>,

    /// Workspace root path.
    pub workspace_root: Option<PathBuf>,

    /// Log entries collected during execution.
    pub logs: Arc<RwLock<Vec<LogEntry>>>,

    /// Start time for timestamp calculation.
    pub start_time: std::time::Instant,
}

impl HostState {
    /// Create a new host state.
    #[must_use]
    pub fn new(
        capabilities: Arc<Capabilities>,
        http_allowlist: SharedAllowlistValidator,
        workspace_root: Option<PathBuf>,
    ) -> Self {
        Self {
            capabilities,
            http_allowlist,
            tool_manager: None,
            secrets: Arc::new(RwLock::new(HashMap::new())),
            workspace_root,
            logs: Arc::new(RwLock::new(Vec::new())),
            start_time: std::time::Instant::now(),
        }
    }

    /// Add a tool manager for tool invocation capability.
    #[must_use]
    pub fn with_tool_manager(mut self, manager: Arc<ToolManager>) -> Self {
        self.tool_manager = Some(manager);
        self
    }

    /// Add secrets to the store.
    pub async fn add_secrets(&self, secrets: HashMap<String, String>) {
        let mut store = self.secrets.write().await;
        store.extend(secrets);
    }

    /// Log a message from the WASM tool.
    pub async fn log(&self, level: LogLevel, message: String) {
        let timestamp_ms = self.start_time.elapsed().as_millis() as u64;
        let entry = LogEntry {
            level,
            message,
            timestamp_ms,
        };

        // Log to tracing
        match level {
            LogLevel::Debug => tracing::debug!("[WASM] {}", entry.message),
            LogLevel::Info => tracing::info!("[WASM] {}", entry.message),
            LogLevel::Warn => tracing::warn!("[WASM] {}", entry.message),
            LogLevel::Error => tracing::error!("[WASM] {}", entry.message),
        }

        // Store the log entry
        let mut logs = self.logs.write().await;
        logs.push(entry);
    }

    /// Get all log entries.
    pub async fn get_logs(&self) -> Vec<LogEntry> {
        self.logs.read().await.clone()
    }

    /// Check if HTTP is allowed for a given URL.
    pub fn is_http_allowed(&self, url: &str) -> bool {
        if !self.capabilities.has_http() {
            return false;
        }
        self.http_allowlist.is_allowed(url)
    }

    /// Check if workspace read is allowed.
    #[must_use]
    pub fn is_workspace_read_allowed(&self) -> bool {
        self.capabilities.has_workspace_read()
    }

    /// Check if a path is within allowed workspace paths.
    pub fn is_path_allowed(&self, path: &Path) -> bool {
        let allowed_paths = self.capabilities.allowed_workspace_paths();

        // If no specific paths are configured, allow if workspace read is granted
        if allowed_paths.is_empty() {
            return self.capabilities.has_workspace_read();
        }

        // Otherwise, check if the path is within any allowed prefix
        allowed_paths.iter().any(|prefix| {
            let prefix_path = PathBuf::from(prefix);
            path.starts_with(&prefix_path)
        })
    }

    /// Check if tool invocation is allowed for a specific tool.
    #[must_use]
    pub fn is_tool_invoke_allowed(&self, tool_name: &str) -> bool {
        self.capabilities.is_tool_allowed(tool_name)
    }

    /// Check if secret access is allowed.
    #[must_use]
    pub fn is_secret_access_allowed(&self) -> bool {
        self.capabilities.has_secret()
    }

    /// Check if a specific secret is allowed.
    #[must_use]
    pub fn is_secret_allowed(&self, name: &str) -> bool {
        self.capabilities.is_secret_allowed(name)
    }

    /// Get a secret value.
    pub async fn get_secret(&self, name: &str) -> Option<String> {
        if !self.is_secret_allowed(name) {
            return None;
        }
        let store = self.secrets.read().await;
        store.get(name).cloned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::wasm::allowlist::AllowlistValidator;
    use crate::tool::wasm::capabilities::Capabilities;

    fn create_test_host_state() -> HostState {
        let caps = Arc::new(Capabilities::default());
        let validator = Arc::new(AllowlistValidator::new(&[]));
        HostState::new(caps, validator, None)
    }

    #[tokio::test]
    async fn log_stores_entries() {
        let state = create_test_host_state();

        state.log(LogLevel::Info, "Test message".to_string()).await;

        let logs = state.get_logs().await;
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].level, LogLevel::Info);
        assert_eq!(logs[0].message, "Test message");
    }

    #[tokio::test]
    async fn add_and_get_secrets() {
        let caps = Arc::new(Capabilities::default().with_secrets(vec!["api_key".to_string()]));
        let validator = Arc::new(AllowlistValidator::new(&[]));
        let state = HostState::new(caps, validator, None);

        let mut secrets = HashMap::new();
        secrets.insert("api_key".to_string(), "secret123".to_string());
        state.add_secrets(secrets).await;

        assert_eq!(
            state.get_secret("api_key").await,
            Some("secret123".to_string())
        );
        assert_eq!(state.get_secret("unknown").await, None);
    }

    #[test]
    fn http_allowed_with_capabilities() {
        let caps = Arc::new(
            Capabilities::default().with_http(vec!["https://api.example.com/*".to_string()]),
        );
        let validator = Arc::new(AllowlistValidator::new(&[
            "https://api.example.com/*".to_string()
        ]));
        let state = HostState::new(caps, validator, None);

        assert!(state.is_http_allowed("https://api.example.com/v1"));
        assert!(!state.is_http_allowed("https://other.example.com/v1"));
    }

    #[test]
    fn path_allowed_check() {
        let caps =
            Arc::new(Capabilities::default().with_workspace_read(vec!["/workspace".to_string()]));
        let validator = Arc::new(AllowlistValidator::new(&[]));
        let state = HostState::new(caps, validator, Some(PathBuf::from("/workspace")));

        assert!(state.is_path_allowed(&PathBuf::from("/workspace/src/main.rs")));
        assert!(!state.is_path_allowed(&PathBuf::from("/etc/passwd")));
    }

    #[test]
    fn tool_invoke_allowed() {
        let caps = Arc::new(Capabilities::default().with_tool_invoke(vec!["echo".to_string()]));
        let validator = Arc::new(AllowlistValidator::new(&[]));
        let state = HostState::new(caps, validator, None);

        assert!(state.is_tool_invoke_allowed("echo"));
        assert!(!state.is_tool_invoke_allowed("shell"));
    }
}
