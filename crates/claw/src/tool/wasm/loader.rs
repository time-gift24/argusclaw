//! WASM tool loader for discovering and loading tools.
//!
//! This module provides functionality for discovering WASM tools
//! from a directory and registering them with the ToolManager.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use super::capabilities_schema::{ToolMetadata, default_metadata};
use super::error::WasmError;
use super::runtime::{DEFAULT_TOOL_SCHEMA, WasmToolRuntime};
use super::wrapper::WasmToolWrapper;
use crate::tool::ToolManager;

/// Default tool directory relative to the data directory.
pub const DEFAULT_TOOLS_DIR: &str = "tools";

/// Loader for WASM tools.
///
/// This struct handles discovering WASM tools from directories,
/// loading their metadata, and registering them with the ToolManager.
#[derive(Debug)]
pub struct WasmToolLoader {
    /// WASM runtime for compiling modules.
    runtime: Arc<WasmToolRuntime>,
    /// Tool manager for registration.
    tool_manager: Arc<ToolManager>,
    /// Root directory for WASM tools.
    tools_dir: PathBuf,
}

impl WasmToolLoader {
    /// Create a new WASM tool loader.
    pub fn new(
        runtime: Arc<WasmToolRuntime>,
        tool_manager: Arc<ToolManager>,
        tools_dir: PathBuf,
    ) -> Self {
        Self {
            runtime,
            tool_manager,
            tools_dir,
        }
    }

    /// Create a loader with the default tools directory.
    pub fn with_default_dir(
        runtime: Arc<WasmToolRuntime>,
        tool_manager: Arc<ToolManager>,
    ) -> Result<Self, WasmError> {
        let data_dir = dirs::data_local_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("argusclaw");

        let tools_dir = data_dir.join(DEFAULT_TOOLS_DIR);

        Ok(Self::new(runtime, tool_manager, tools_dir))
    }

    /// Get the tools directory path.
    #[must_use]
    pub fn tools_dir(&self) -> &Path {
        &self.tools_dir
    }

    /// Ensure the tools directory exists.
    pub fn ensure_tools_dir(&self) -> Result<(), WasmError> {
        if !self.tools_dir.exists() {
            std::fs::create_dir_all(&self.tools_dir).map_err(|e| {
                WasmError::ToolDirectoryNotFound {
                    path: self.tools_dir.clone(),
                    reason: e.to_string(),
                }
            })?;
            tracing::info!("Created tools directory: {:?}", self.tools_dir);
        }
        Ok(())
    }

    /// Discover all WASM tools in the tools directory.
    ///
    /// Returns a list of paths to WASM files.
    pub fn discover_tools(&self) -> Result<Vec<PathBuf>, WasmError> {
        self.ensure_tools_dir()?;

        let mut wasm_files = Vec::new();

        let entries =
            std::fs::read_dir(&self.tools_dir).map_err(|e| WasmError::ToolDirectoryNotFound {
                path: self.tools_dir.clone(),
                reason: e.to_string(),
            })?;

        for entry in entries {
            let entry = entry.map_err(WasmError::Io)?;
            let path = entry.path();

            if path.extension().is_some_and(|ext| ext == "wasm") {
                wasm_files.push(path);
            }
        }

        tracing::info!("Discovered {} WASM tools", wasm_files.len());
        Ok(wasm_files)
    }

    /// Load a single WASM tool and register it with the tool manager.
    pub fn load_tool(&self, wasm_path: &Path) -> Result<(), WasmError> {
        tracing::info!("Loading WASM tool from: {:?}", wasm_path);

        // Compile the module
        let module = self.runtime.prepare_module(wasm_path)?;

        // Try to load metadata from capabilities file
        let metadata = ToolMetadata::try_for_wasm_file(wasm_path)?;

        // Use metadata or defaults
        let (name, description, schema, capabilities) = if let Some(meta) = metadata {
            tracing::debug!(
                "Loaded metadata for tool '{}': {}",
                meta.name,
                meta.description
            );
            let caps = Arc::new(meta.capabilities);
            (
                meta.name,
                meta.description,
                serde_json::to_string(&caps).unwrap_or_default(),
                caps,
            )
        } else {
            // Use defaults based on filename
            let name = wasm_path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("unknown")
                .to_string();

            tracing::debug!("No capabilities file found, using defaults for '{}'", name);
            let meta = default_metadata(&name);
            let caps = Arc::new(meta.capabilities);
            (
                meta.name,
                meta.description,
                DEFAULT_TOOL_SCHEMA.to_string(),
                caps,
            )
        };

        // Apply capabilities to the module
        let module = module.with_capabilities(capabilities);

        // Create the wrapper
        let wrapper = WasmToolWrapper::with_metadata(
            module,
            wasm_path.to_path_buf(),
            name.clone(),
            description,
            schema,
        );

        // Register with tool manager
        self.tool_manager.register(Arc::new(wrapper));

        tracing::info!("Registered WASM tool: {}", name);
        Ok(())
    }

    /// Load all discovered WASM tools.
    ///
    /// Returns the number of successfully loaded tools and any errors.
    pub fn load_all(&self) -> (usize, Vec<(PathBuf, WasmError)>) {
        let tools = match self.discover_tools() {
            Ok(t) => t,
            Err(e) => {
                tracing::error!("Failed to discover tools: {}", e);
                return (0, vec![(self.tools_dir.clone(), e)]);
            }
        };

        let mut loaded = 0;
        let mut errors = Vec::new();

        for tool_path in tools {
            match self.load_tool(&tool_path) {
                Ok(()) => loaded += 1,
                Err(e) => {
                    tracing::error!("Failed to load tool {:?}: {}", tool_path, e);
                    errors.push((tool_path, e));
                }
            }
        }

        tracing::info!("Loaded {} WASM tools ({} errors)", loaded, errors.len());
        (loaded, errors)
    }
}

/// Discover and load all WASM tools from the default directory.
///
/// This is a convenience function for the common use case.
pub fn load_wasm_tools(
    runtime: Arc<WasmToolRuntime>,
    tool_manager: Arc<ToolManager>,
) -> Result<(usize, Vec<(PathBuf, WasmError)>), WasmError> {
    let loader = WasmToolLoader::with_default_dir(runtime, tool_manager)?;
    Ok(loader.load_all())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn loader_creation() {
        let runtime = Arc::new(WasmToolRuntime::new().unwrap());
        let manager = Arc::new(ToolManager::new());
        let loader = WasmToolLoader::new(runtime, manager, PathBuf::from("/tmp/tools"));

        assert_eq!(loader.tools_dir(), Path::new("/tmp/tools"));
    }

    #[test]
    fn discover_empty_directory() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let runtime = Arc::new(WasmToolRuntime::new().unwrap());
        let manager = Arc::new(ToolManager::new());
        let loader = WasmToolLoader::new(runtime, manager, temp_dir.path().to_path_buf());

        let tools = loader.discover_tools().unwrap();
        assert!(tools.is_empty());
    }

    #[test]
    fn discover_wasm_files() {
        use std::io::Write;

        let temp_dir = tempfile::TempDir::new().unwrap();

        // Create some test files
        std::fs::File::create(temp_dir.path().join("tool1.wasm"))
            .unwrap()
            .write_all(b"mock wasm")
            .unwrap();
        std::fs::File::create(temp_dir.path().join("tool2.wasm"))
            .unwrap()
            .write_all(b"mock wasm")
            .unwrap();
        std::fs::File::create(temp_dir.path().join("readme.txt"))
            .unwrap()
            .write_all(b"not a wasm")
            .unwrap();

        let runtime = Arc::new(WasmToolRuntime::new().unwrap());
        let manager = Arc::new(ToolManager::new());
        let loader = WasmToolLoader::new(runtime, manager, temp_dir.path().to_path_buf());

        let tools = loader.discover_tools().unwrap();
        assert_eq!(tools.len(), 2);
    }
}
