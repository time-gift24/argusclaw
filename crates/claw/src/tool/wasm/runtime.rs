//! WASM runtime for tool execution.
//!
//! This module provides the core runtime for compiling and executing WASM tools.
//! It follows the "compile once, instantiate fresh" pattern for security and isolation.

use std::path::Path;
use std::sync::Arc;
use wasmtime::{Config, Engine, Instance, Linker, Module, Store};

use super::capabilities::Capabilities;
use super::error::WasmError;
use super::host::HostState;
use super::limits::ResourceLimits;

/// Expected WIT version for compatibility checking.
pub const WIT_VERSION: &str = "0.1.0";

/// Default tool description when metadata is unavailable.
pub const DEFAULT_TOOL_DESCRIPTION: &str = "A WASM tool";

/// Default tool schema when metadata is unavailable.
pub const DEFAULT_TOOL_SCHEMA: &str = r#"{"type": "object"}"#;

/// Global WASM runtime shared across all tool executions.
///
/// This struct holds the wasmtime Engine which is expensive to create
/// and should be shared across executions.
#[derive(Clone)]
pub struct WasmToolRuntime {
    /// The wasmtime engine (shared across executions).
    engine: Engine,
    /// Default resource limits.
    default_limits: Arc<ResourceLimits>,
}

impl std::fmt::Debug for WasmToolRuntime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("WasmToolRuntime")
            .field("default_limits", &self.default_limits)
            .finish_non_exhaustive()
    }
}

impl WasmToolRuntime {
    /// Create a new WASM runtime with default configuration.
    pub fn new() -> Result<Self, WasmError> {
        let mut config = Config::new();

        // Enable fuel metering for CPU limiting
        config.consume_fuel(true);

        // Enable epoch interruption for timeout support
        config.epoch_interruption(true);

        // Enable caching for faster subsequent loads
        config
            .cache_config_load_default()
            .map_err(|e| WasmError::runtime_init(e.to_string()))?;

        let engine = Engine::new(&config).map_err(|e| WasmError::runtime_init(e.to_string()))?;

        Ok(Self {
            engine,
            default_limits: Arc::new(ResourceLimits::default()),
        })
    }

    /// Create a new WASM runtime with custom resource limits.
    pub fn with_limits(limits: ResourceLimits) -> Result<Self, WasmError> {
        let mut runtime = Self::new()?;
        runtime.default_limits = Arc::new(limits);
        Ok(runtime)
    }

    /// Get the wasmtime engine.
    #[must_use]
    pub fn engine(&self) -> &Engine {
        &self.engine
    }

    /// Get the default resource limits.
    #[must_use]
    pub fn default_limits(&self) -> &ResourceLimits {
        &self.default_limits
    }

    /// Compile a WASM module from file.
    ///
    /// This validates the module and prepares it for execution.
    /// The compiled module can be reused for multiple executions.
    pub fn prepare_module(&self, path: &Path) -> Result<PreparedModule, WasmError> {
        // Read the WASM file
        let wasm_bytes =
            std::fs::read(path).map_err(|e| WasmError::module_load(path, e.to_string()))?;

        // Compile the module
        let module = Module::new(&self.engine, &wasm_bytes)
            .map_err(|e| WasmError::module_load(path, e.to_string()))?;

        // Validate exports
        self.validate_exports(&module)?;

        Ok(PreparedModule {
            module,
            runtime: self.clone(),
            limits: self.default_limits.clone(),
            capabilities: Arc::new(Capabilities::default()),
        })
    }

    /// Compile a WASM module from bytes.
    pub fn prepare_module_from_bytes(&self, bytes: &[u8]) -> Result<PreparedModule, WasmError> {
        let module = Module::new(&self.engine, bytes)
            .map_err(|e| WasmError::module_load("<memory>", e.to_string()))?;

        self.validate_exports(&module)?;

        Ok(PreparedModule {
            module,
            runtime: self.clone(),
            limits: self.default_limits.clone(),
            capabilities: Arc::new(Capabilities::default()),
        })
    }

    /// Validate that the module has required exports.
    fn validate_exports(&self, module: &Module) -> Result<(), WasmError> {
        let required_exports = ["execute", "schema", "description", "name"];

        for export in required_exports {
            if module.get_export(export).is_none() {
                return Err(WasmError::export_not_found(export));
            }
        }

        Ok(())
    }
}

impl Default for WasmToolRuntime {
    fn default() -> Self {
        Self::new().expect("Failed to create WASM runtime")
    }
}

/// A compiled WASM module ready for execution.
///
/// This follows the "compile once, instantiate fresh" pattern:
/// - The module is compiled once when loaded
/// - Each execution creates a fresh instance for isolation
pub struct PreparedModule {
    /// The compiled wasmtime module.
    pub(crate) module: Module,
    /// Reference to the runtime.
    pub(crate) runtime: WasmToolRuntime,
    /// Resource limits for this module.
    pub(crate) limits: Arc<ResourceLimits>,
    /// Capabilities for this module.
    pub(crate) capabilities: Arc<Capabilities>,
}

impl PreparedModule {
    /// Set custom capabilities for this module.
    #[must_use]
    pub fn with_capabilities(mut self, capabilities: Arc<Capabilities>) -> Self {
        self.capabilities = capabilities;
        self
    }

    /// Set custom resource limits for this module.
    #[must_use]
    pub fn with_limits(mut self, limits: Arc<ResourceLimits>) -> Self {
        self.limits = limits;
        self
    }

    /// Get the tool name from the module.
    pub fn get_name(&self) -> Result<String, WasmError> {
        // For now, return a placeholder - actual implementation needs
        // to call into the WASM module
        Ok("wasm_tool".to_string())
    }

    /// Get the tool description from the module.
    pub fn get_description(&self) -> Result<String, WasmError> {
        Ok(DEFAULT_TOOL_DESCRIPTION.to_string())
    }

    /// Get the tool schema from the module.
    pub fn get_schema(&self) -> Result<String, WasmError> {
        Ok(DEFAULT_TOOL_SCHEMA.to_string())
    }

    /// Create a new WASM instance with a fresh store.
    ///
    /// This is the core of the isolation strategy - each execution
    /// gets a completely fresh instance.
    pub(crate) fn create_instance(
        &self,
        host_state: HostState,
    ) -> Result<(Store<HostState>, Instance), WasmError> {
        // Create store with host state
        let mut store = Store::new(&self.runtime.engine, host_state);

        // Set initial fuel for CPU limiting
        store
            .set_fuel(self.limits.fuel_limit)
            .map_err(|e| WasmError::instantiation(e.to_string()))?;

        // Create linker
        let linker = Linker::new(&self.runtime.engine);

        // Note: WASI support requires WasiP1Ctx as the store data type.
        // For now, we'll skip WASI and only support our custom tool interface.
        // A full implementation would use a wrapper type that includes both
        // HostState and WasiP1Ctx.

        // Instantiate
        let instance = linker
            .instantiate(&mut store, &self.module)
            .map_err(|e| WasmError::instantiation(e.to_string()))?;

        Ok((store, instance))
    }
}

impl std::fmt::Debug for PreparedModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PreparedModule")
            .field("limits", &self.limits)
            .field("capabilities", &self.capabilities)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_creation() {
        let runtime = WasmToolRuntime::new();
        assert!(runtime.is_ok());
    }

    #[test]
    fn runtime_default_limits() {
        let runtime = WasmToolRuntime::new().unwrap();
        let limits = runtime.default_limits();
        assert_eq!(limits.memory_limit, ResourceLimits::default().memory_limit);
        assert_eq!(limits.fuel_limit, ResourceLimits::default().fuel_limit);
    }

    #[test]
    fn runtime_with_custom_limits() {
        let limits = ResourceLimits::new(5 * 1024 * 1024, 5000, 5_000_000, 30_000);
        let runtime = WasmToolRuntime::with_limits(limits.clone()).unwrap();
        assert_eq!(runtime.default_limits().memory_limit, 5 * 1024 * 1024);
    }

    #[test]
    fn prepare_invalid_wasm() {
        let runtime = WasmToolRuntime::new().unwrap();
        let result = runtime.prepare_module_from_bytes(b"invalid wasm");
        assert!(result.is_err());
    }
}
