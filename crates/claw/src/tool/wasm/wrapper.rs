//! WASM tool wrapper that implements NamedTool trait.
//!
//! This module provides the bridge between WASM tools and the
//! Argusclaw tool system.

use async_trait::async_trait;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::time::timeout;

use super::capabilities::Capabilities;
use super::error::WasmError;
use super::host::HostState;
use super::limits::ResourceLimits;
use super::runtime::{DEFAULT_TOOL_DESCRIPTION, DEFAULT_TOOL_SCHEMA, PreparedModule};
use crate::llm::ToolDefinition;
use crate::protocol::RiskLevel;
use crate::tool::{NamedTool, ToolError};

/// A WASM tool wrapped to implement NamedTool.
///
/// This struct wraps a compiled WASM module and provides the
/// NamedTool interface for integration with Argusclaw.
#[derive(Debug)]
pub struct WasmToolWrapper {
    /// The prepared WASM module.
    module: PreparedModule,
    /// Tool name (cached for performance).
    name: String,
    /// Tool description (cached for performance).
    description: String,
    /// Tool schema (cached for performance).
    schema: String,
    /// Path to the WASM file (for debugging).
    path: PathBuf,
    /// Capabilities for this tool.
    capabilities: Arc<Capabilities>,
    /// Resource limits for this tool.
    limits: Arc<ResourceLimits>,
}

impl WasmToolWrapper {
    /// Create a new WASM tool wrapper from a prepared module.
    pub fn new(module: PreparedModule, path: PathBuf) -> Result<Self, WasmError> {
        // Get tool metadata from the module
        // Note: In a real implementation, we'd extract these from the WASM
        // For now, we use placeholder values that can be overridden
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("wasm_tool")
            .to_string();

        let capabilities = module.capabilities.clone();
        let limits = module.limits.clone();

        Ok(Self {
            module,
            name,
            description: DEFAULT_TOOL_DESCRIPTION.to_string(),
            schema: DEFAULT_TOOL_SCHEMA.to_string(),
            path,
            capabilities,
            limits,
        })
    }

    /// Create a new WASM tool wrapper with explicit metadata.
    pub fn with_metadata(
        module: PreparedModule,
        path: PathBuf,
        name: String,
        description: String,
        schema: String,
    ) -> Self {
        let capabilities = module.capabilities.clone();
        let limits = module.limits.clone();

        Self {
            module,
            name,
            description,
            schema,
            path,
            capabilities,
            limits,
        }
    }

    /// Get the path to the WASM file.
    #[must_use]
    pub fn path(&self) -> &PathBuf {
        &self.path
    }

    /// Get the capabilities of this tool.
    #[must_use]
    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    /// Get the resource limits of this tool.
    #[must_use]
    pub fn limits(&self) -> &ResourceLimits {
        &self.limits
    }

    /// Execute the WASM tool with the given parameters.
    ///
    /// This creates a fresh WASM instance for each execution,
    /// ensuring complete isolation between runs.
    async fn execute_wasm(
        &self,
        params: serde_json::Value,
    ) -> Result<serde_json::Value, WasmError> {
        let params_json = serde_json::to_string(&params)?;

        // Create execution context
        let execution = WasmExecution {
            module: &self.module,
            params_json,
            timeout_ms: self.limits.timeout_ms,
        };

        // Execute with timeout
        let result = timeout(
            std::time::Duration::from_millis(self.limits.timeout_ms),
            execution.run(),
        )
        .await
        .map_err(|_| WasmError::timeout(self.limits.timeout_ms))??;

        Ok(result)
    }
}

/// A single WASM execution context.
struct WasmExecution<'a> {
    module: &'a PreparedModule,
    params_json: String,
    #[allow(dead_code)]
    timeout_ms: u64,
}

impl<'a> WasmExecution<'a> {
    /// Run the WASM execution.
    async fn run(self) -> Result<serde_json::Value, WasmError> {
        // Create host state for this execution
        let host_state = HostState::new(
            self.module.capabilities.clone(),
            std::sync::Arc::new(super::allowlist::AllowlistValidator::new(
                self.module.capabilities.allowed_http_endpoints(),
            )),
            None,
        );

        // Create a fresh instance for this execution
        let (_store, _instance) = self.module.create_instance(host_state)?;

        // For now, return a placeholder response
        // A full implementation would:
        // 1. Get the execute function from the instance
        // 2. Call it with the parameters
        // 3. Return the result

        let result = serde_json::json!({
            "success": true,
            "message": "WASM execution placeholder",
            "params": self.params_json
        });

        Ok(result)
    }
}

#[async_trait]
impl NamedTool for WasmToolWrapper {
    fn name(&self) -> &str {
        &self.name
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: self.name.clone(),
            description: self.description.clone(),
            parameters: serde_json::from_str(&self.schema).unwrap_or(serde_json::json!({
                "type": "object"
            })),
        }
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        self.execute_wasm(args)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool_name: self.name.clone(),
                reason: e.to_string(),
            })
    }

    fn risk_level(&self) -> RiskLevel {
        // Determine risk level based on capabilities
        if self.capabilities.has_tool_invoke() || self.capabilities.has_secret() {
            RiskLevel::High
        } else if self.capabilities.has_http() {
            RiskLevel::Medium
        } else {
            // Default to Low for read-only or no capabilities
            RiskLevel::Low
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require actual WASM modules to run properly.
    // For now, we test the wrapper structure without actual execution.

    #[test]
    fn risk_level_based_on_capabilities() {
        // Low risk: no capabilities
        let caps = Arc::new(Capabilities::default());
        assert_eq!(determine_risk_level(&caps), RiskLevel::Low);

        // Medium risk: HTTP capability
        let caps = Arc::new(
            Capabilities::default().with_http(vec!["https://api.example.com/*".to_string()]),
        );
        assert_eq!(determine_risk_level(&caps), RiskLevel::Medium);

        // High risk: tool invoke capability
        let caps = Arc::new(Capabilities::default().with_tool_invoke(vec!["shell".to_string()]));
        assert_eq!(determine_risk_level(&caps), RiskLevel::High);

        // High risk: secret access
        let caps = Arc::new(Capabilities::default().with_secrets(vec!["api_key".to_string()]));
        assert_eq!(determine_risk_level(&caps), RiskLevel::High);
    }

    fn determine_risk_level(capabilities: &Capabilities) -> RiskLevel {
        if capabilities.has_tool_invoke() || capabilities.has_secret() {
            RiskLevel::High
        } else if capabilities.has_http() {
            RiskLevel::Medium
        } else {
            RiskLevel::Low
        }
    }
}
