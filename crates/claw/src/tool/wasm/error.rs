//! WASM tool error types.

use std::path::PathBuf;
use thiserror::Error;

/// Error type for WASM tool operations.
#[derive(Debug, Error)]
pub enum WasmError {
    /// Failed to initialize the WASM runtime.
    #[error("Failed to initialize WASM runtime: {reason}")]
    RuntimeInit { reason: String },

    /// Failed to load or compile the WASM module.
    #[error("Failed to load WASM module from '{path}': {reason}")]
    ModuleLoad { path: PathBuf, reason: String },

    /// Failed to instantiate the WASM module.
    #[error("Failed to instantiate WASM module: {reason}")]
    Instantiation { reason: String },

    /// Tool export not found in the WASM module.
    #[error("Required export '{export_name}' not found in WASM module")]
    ExportNotFound { export_name: String },

    /// Tool execution failed.
    #[error("WASM tool execution failed: {reason}")]
    Execution { reason: String },

    /// Tool execution exceeded resource limits.
    #[error("Resource limit exceeded: {limit_type} (limit: {limit}, actual: {actual})")]
    ResourceLimitExceeded {
        limit_type: String,
        limit: u64,
        actual: u64,
    },

    /// Tool execution timed out.
    #[error("WASM tool execution timed out after {timeout_ms}ms")]
    Timeout { timeout_ms: u64 },

    /// Invalid tool output.
    #[error("Invalid tool output: {reason}")]
    InvalidOutput { reason: String },

    /// Host function call failed.
    #[error("Host function '{function}' failed: {reason}")]
    HostFunction { function: String, reason: String },

    /// Capability not granted.
    #[error("Capability '{capability}' not granted for this tool")]
    CapabilityNotGranted { capability: String },

    /// HTTP request blocked by allowlist.
    #[error("HTTP request to '{url}' blocked by allowlist")]
    HttpBlocked { url: String },

    /// Secret not found.
    #[error("Secret '{name}' not found")]
    SecretNotFound { name: String },

    /// Failed to read capabilities file.
    #[error("Failed to read capabilities file '{path}': {reason}")]
    CapabilitiesRead { path: PathBuf, reason: String },

    /// Invalid capabilities JSON.
    #[error("Invalid capabilities JSON: {reason}")]
    InvalidCapabilities { reason: String },

    /// Tool directory not found.
    #[error("Tool directory not found: {path}")]
    ToolDirectoryNotFound { path: PathBuf },

    /// WIT version mismatch.
    #[error("WIT version mismatch: expected '{expected}', got '{actual}'")]
    WitVersionMismatch { expected: String, actual: String },

    /// IO error.
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON error.
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

impl WasmError {
    /// Create a new runtime initialization error.
    pub fn runtime_init(reason: impl Into<String>) -> Self {
        Self::RuntimeInit {
            reason: reason.into(),
        }
    }

    /// Create a new module load error.
    pub fn module_load(path: impl Into<PathBuf>, reason: impl Into<String>) -> Self {
        Self::ModuleLoad {
            path: path.into(),
            reason: reason.into(),
        }
    }

    /// Create a new instantiation error.
    pub fn instantiation(reason: impl Into<String>) -> Self {
        Self::Instantiation {
            reason: reason.into(),
        }
    }

    /// Create a new export not found error.
    pub fn export_not_found(export_name: impl Into<String>) -> Self {
        Self::ExportNotFound {
            export_name: export_name.into(),
        }
    }

    /// Create a new execution error.
    pub fn execution(reason: impl Into<String>) -> Self {
        Self::Execution {
            reason: reason.into(),
        }
    }

    /// Create a new timeout error.
    pub fn timeout(timeout_ms: u64) -> Self {
        Self::Timeout { timeout_ms }
    }

    /// Create a new invalid output error.
    pub fn invalid_output(reason: impl Into<String>) -> Self {
        Self::InvalidOutput {
            reason: reason.into(),
        }
    }

    /// Create a new host function error.
    pub fn host_function(function: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::HostFunction {
            function: function.into(),
            reason: reason.into(),
        }
    }

    /// Create a new capability not granted error.
    pub fn capability_not_granted(capability: impl Into<String>) -> Self {
        Self::CapabilityNotGranted {
            capability: capability.into(),
        }
    }

    /// Create a new HTTP blocked error.
    pub fn http_blocked(url: impl Into<String>) -> Self {
        Self::HttpBlocked { url: url.into() }
    }
}
