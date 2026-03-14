//! WASM tool capabilities system.
//!
//! This module defines the opt-in capability system for WASM tools.
//! All capabilities are denied by default and must be explicitly granted.

use serde::{Deserialize, Serialize};

/// HTTP capability configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct HttpCapability {
    /// Allowed URL patterns (supports wildcards).
    /// Example: ["https://api.example.com/*", "https://cdn.example.com/**"]
    #[serde(default)]
    pub allowed_endpoints: Vec<String>,

    /// Maximum request timeout in milliseconds.
    #[serde(default = "default_http_timeout")]
    pub timeout_ms: u64,

    /// Maximum response body size in bytes.
    #[serde(default = "default_max_response_size")]
    pub max_response_size: usize,
}

fn default_http_timeout() -> u64 {
    30_000 // 30 seconds
}

fn default_max_response_size() -> usize {
    10 * 1024 * 1024 // 10 MB
}

/// Workspace (file system) capability configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkspaceCapability {
    /// Whether read access is allowed.
    #[serde(default)]
    pub read: bool,

    /// Allowed path prefixes for reading.
    #[serde(default)]
    pub allowed_paths: Vec<String>,
}

/// Tool invocation capability configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ToolInvokeCapability {
    /// Whether tool invocation is allowed.
    #[serde(default)]
    pub enabled: bool,

    /// List of tool names/aliases that can be invoked.
    /// Empty means all tools are allowed (if enabled).
    #[serde(default)]
    pub allowed_tools: Vec<String>,
}

/// Secret access capability configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SecretCapability {
    /// Whether secret access is allowed.
    #[serde(default)]
    pub enabled: bool,

    /// List of secret names that can be accessed.
    #[serde(default)]
    pub allowed_secrets: Vec<String>,
}

/// Complete capabilities configuration for a WASM tool.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Capabilities {
    /// HTTP request capability.
    #[serde(default)]
    pub http: Option<HttpCapability>,

    /// Workspace/file system capability.
    #[serde(default)]
    pub workspace: Option<WorkspaceCapability>,

    /// Tool invocation capability.
    #[serde(default)]
    pub tool_invoke: Option<ToolInvokeCapability>,

    /// Secret access capability.
    #[serde(default)]
    pub secret: Option<SecretCapability>,

    /// Custom resource limits (overrides defaults).
    #[serde(default)]
    pub resource_limits: Option<ResourceLimitsConfig>,
}

/// Resource limits configuration from JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceLimitsConfig {
    /// Maximum memory in bytes.
    #[serde(default = "default_memory_limit")]
    pub memory_limit: usize,

    /// Maximum fuel (CPU instructions).
    #[serde(default = "default_fuel_limit")]
    pub fuel_limit: u64,

    /// Execution timeout in milliseconds.
    #[serde(default = "default_timeout")]
    pub timeout_ms: u64,
}

fn default_memory_limit() -> usize {
    super::limits::DEFAULT_MEMORY_LIMIT
}

fn default_fuel_limit() -> u64 {
    super::limits::DEFAULT_FUEL_LIMIT
}

fn default_timeout() -> u64 {
    super::limits::DEFAULT_TIMEOUT_MS
}

impl Default for ResourceLimitsConfig {
    fn default() -> Self {
        Self {
            memory_limit: default_memory_limit(),
            fuel_limit: default_fuel_limit(),
            timeout_ms: default_timeout(),
        }
    }
}

impl Capabilities {
    /// Create capabilities with HTTP access.
    #[must_use]
    pub fn with_http(mut self, allowed_endpoints: Vec<String>) -> Self {
        self.http = Some(HttpCapability {
            allowed_endpoints,
            ..Default::default()
        });
        self
    }

    /// Create capabilities with workspace read access.
    #[must_use]
    pub fn with_workspace_read(mut self, allowed_paths: Vec<String>) -> Self {
        self.workspace = Some(WorkspaceCapability {
            read: true,
            allowed_paths,
        });
        self
    }

    /// Create capabilities with tool invocation.
    #[must_use]
    pub fn with_tool_invoke(mut self, allowed_tools: Vec<String>) -> Self {
        self.tool_invoke = Some(ToolInvokeCapability {
            enabled: true,
            allowed_tools,
        });
        self
    }

    /// Create capabilities with secret access.
    #[must_use]
    pub fn with_secrets(mut self, allowed_secrets: Vec<String>) -> Self {
        self.secret = Some(SecretCapability {
            enabled: true,
            allowed_secrets,
        });
        self
    }

    /// Check if HTTP capability is granted.
    #[must_use]
    pub fn has_http(&self) -> bool {
        self.http.is_some()
    }

    /// Check if workspace read is granted.
    #[must_use]
    pub fn has_workspace_read(&self) -> bool {
        self.workspace.as_ref().is_some_and(|w| w.read)
    }

    /// Check if tool invocation is granted.
    #[must_use]
    pub fn has_tool_invoke(&self) -> bool {
        self.tool_invoke.as_ref().is_some_and(|t| t.enabled)
    }

    /// Check if secret access is granted.
    #[must_use]
    pub fn has_secret(&self) -> bool {
        self.secret.as_ref().is_some_and(|s| s.enabled)
    }

    /// Check if a specific secret is allowed.
    #[must_use]
    pub fn is_secret_allowed(&self, name: &str) -> bool {
        if let Some(ref secret) = self.secret
            && secret.enabled
        {
            return secret.allowed_secrets.is_empty()
                || secret.allowed_secrets.iter().any(|s| s == name);
        }
        false
    }

    /// Check if a specific tool is allowed to be invoked.
    #[must_use]
    pub fn is_tool_allowed(&self, name: &str) -> bool {
        if let Some(ref tool_invoke) = self.tool_invoke
            && tool_invoke.enabled
        {
            return tool_invoke.allowed_tools.is_empty()
                || tool_invoke.allowed_tools.iter().any(|t| t == name);
        }
        false
    }

    /// Get allowed HTTP endpoints.
    #[must_use]
    pub fn allowed_http_endpoints(&self) -> &[String] {
        self.http
            .as_ref()
            .map(|h| h.allowed_endpoints.as_slice())
            .unwrap_or(&[])
    }

    /// Get allowed workspace paths.
    #[must_use]
    pub fn allowed_workspace_paths(&self) -> &[String] {
        self.workspace
            .as_ref()
            .map(|w| w.allowed_paths.as_slice())
            .unwrap_or(&[])
    }
}

impl ResourceLimitsConfig {
    /// Convert to the runtime ResourceLimits type.
    #[must_use]
    pub fn to_resource_limits(&self) -> super::limits::ResourceLimits {
        super::limits::ResourceLimits::new(
            self.memory_limit,
            super::limits::DEFAULT_TABLE_LIMIT,
            self.fuel_limit,
            self.timeout_ms,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_capabilities_deny_all() {
        let caps = Capabilities::default();
        assert!(!caps.has_http());
        assert!(!caps.has_workspace_read());
        assert!(!caps.has_tool_invoke());
        assert!(!caps.has_secret());
    }

    #[test]
    fn builder_pattern_grants_capabilities() {
        let caps = Capabilities::default()
            .with_http(vec!["https://api.example.com/*".to_string()])
            .with_workspace_read(vec!["/workspace".to_string()])
            .with_tool_invoke(vec!["echo".to_string()])
            .with_secrets(vec!["api_key".to_string()]);

        assert!(caps.has_http());
        assert!(caps.has_workspace_read());
        assert!(caps.has_tool_invoke());
        assert!(caps.has_secret());
    }

    #[test]
    fn secret_allowed_check() {
        let caps = Capabilities::default().with_secrets(vec!["api_key".to_string()]);

        assert!(caps.is_secret_allowed("api_key"));
        assert!(!caps.is_secret_allowed("other_secret"));
    }

    #[test]
    fn secret_allowed_empty_means_all() {
        let caps = Capabilities::default().with_secrets(vec![]);

        // Empty allowed_secrets means all secrets are allowed
        assert!(caps.is_secret_allowed("any_secret"));
    }

    #[test]
    fn tool_allowed_check() {
        let caps = Capabilities::default().with_tool_invoke(vec!["echo".to_string()]);

        assert!(caps.is_tool_allowed("echo"));
        assert!(!caps.is_tool_allowed("shell"));
    }

    #[test]
    fn tool_allowed_empty_means_all() {
        let caps = Capabilities::default().with_tool_invoke(vec![]);

        // Empty allowed_tools means all tools are allowed
        assert!(caps.is_tool_allowed("any_tool"));
    }

    #[test]
    fn serialize_deserialize_capabilities() {
        let caps = Capabilities::default()
            .with_http(vec!["https://api.example.com/*".to_string()])
            .with_workspace_read(vec!["/workspace".to_string()]);

        let json = serde_json::to_string(&caps).unwrap();
        let parsed: Capabilities = serde_json::from_str(&json).unwrap();

        assert!(parsed.has_http());
        assert!(parsed.has_workspace_read());
        assert_eq!(
            parsed.allowed_http_endpoints(),
            &["https://api.example.com/*"]
        );
    }
}
