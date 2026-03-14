//! Capabilities file schema and parsing.
//!
//! This module handles parsing the `<tool>.capabilities.json` files
//! that accompany WASM tools.

use std::path::{Path, PathBuf};

use super::capabilities::Capabilities;
use super::error::WasmError;

/// Metadata about a WASM tool from its capabilities file.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ToolMetadata {
    /// Tool name/identifier.
    pub name: String,

    /// Human-readable description.
    pub description: String,

    /// Tool version.
    #[serde(default)]
    pub version: String,

    /// WIT interface version this tool is compatible with.
    #[serde(default = "default_wit_version")]
    pub wit_version: String,

    /// Tool capabilities.
    #[serde(default)]
    pub capabilities: Capabilities,

    /// Risk level override (low, medium, high, critical).
    #[serde(default)]
    pub risk_level: Option<String>,

    /// Author/maintainer information.
    #[serde(default)]
    pub author: Option<String>,

    /// Homepage or documentation URL.
    #[serde(default)]
    pub homepage: Option<String>,
}

fn default_wit_version() -> String {
    super::WIT_VERSION.to_string()
}

impl ToolMetadata {
    /// Load tool metadata from a capabilities file.
    ///
    /// The capabilities file should be named `<tool>.capabilities.json`
    /// and located in the same directory as the WASM file.
    pub fn from_file(path: &Path) -> Result<Self, WasmError> {
        let content = std::fs::read_to_string(path).map_err(|e| WasmError::CapabilitiesRead {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;

        let metadata: Self =
            serde_json::from_str(&content).map_err(|e| WasmError::InvalidCapabilities {
                reason: e.to_string(),
            })?;

        // Validate WIT version compatibility
        metadata.validate_wit_version()?;

        Ok(metadata)
    }

    /// Try to load metadata for a WASM file.
    ///
    /// Looks for a `<name>.capabilities.json` file in the same directory.
    /// Returns None if the file doesn't exist.
    pub fn try_for_wasm_file(wasm_path: &Path) -> Result<Option<Self>, WasmError> {
        let caps_path = capabilities_path_for_wasm(wasm_path);

        if caps_path.exists() {
            Self::from_file(&caps_path).map(Some)
        } else {
            Ok(None)
        }
    }

    /// Validate WIT version compatibility.
    fn validate_wit_version(&self) -> Result<(), WasmError> {
        let expected = super::runtime::WIT_VERSION;

        // Simple version comparison - major.minor should match
        let expected_parts: Vec<&str> = expected.split('.').collect();
        let actual_parts: Vec<&str> = self.wit_version.split('.').collect();

        if actual_parts.len() >= 2
            && expected_parts.len() >= 2
            && actual_parts[0] == expected_parts[0]
            && actual_parts[1] == expected_parts[1]
        {
            return Ok(());
        }

        Err(WasmError::WitVersionMismatch {
            expected: expected.to_string(),
            actual: self.wit_version.clone(),
        })
    }

    /// Get the risk level from metadata.
    #[must_use]
    pub fn risk_level(&self) -> Option<crate::protocol::RiskLevel> {
        self.risk_level
            .as_ref()
            .and_then(|s| match s.to_lowercase().as_str() {
                "low" => Some(crate::protocol::RiskLevel::Low),
                "medium" => Some(crate::protocol::RiskLevel::Medium),
                "high" => Some(crate::protocol::RiskLevel::High),
                "critical" => Some(crate::protocol::RiskLevel::Critical),
                _ => None,
            })
    }
}

/// Get the capabilities file path for a WASM file.
#[must_use]
pub fn capabilities_path_for_wasm(wasm_path: &Path) -> PathBuf {
    let stem = wasm_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("tool");

    let parent = wasm_path.parent().unwrap_or(Path::new("."));

    parent.join(format!("{}.capabilities.json", stem))
}

/// Default metadata for a WASM tool when no capabilities file exists.
#[must_use]
pub fn default_metadata(name: &str) -> ToolMetadata {
    ToolMetadata {
        name: name.to_string(),
        description: format!("WASM tool: {}", name),
        version: "0.1.0".to_string(),
        wit_version: super::runtime::WIT_VERSION.to_string(),
        capabilities: Capabilities::default(),
        risk_level: None,
        author: None,
        homepage: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tool::wasm::runtime::WIT_VERSION;
    use std::io::Write;
    use tempfile::TempDir;

    #[test]
    fn parse_valid_metadata() {
        let json = r#"{
            "name": "test-tool",
            "description": "A test tool",
            "version": "1.0.0",
            "wit_version": "0.1.0",
            "capabilities": {
                "http": {
                    "allowed_endpoints": ["https://api.example.com/*"]
                }
            }
        }"#;

        let metadata: ToolMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(metadata.name, "test-tool");
        assert_eq!(metadata.description, "A test tool");
        assert_eq!(metadata.version, "1.0.0");
        assert!(metadata.capabilities.has_http());
    }

    #[test]
    fn parse_minimal_metadata() {
        let json = r#"{
            "name": "minimal",
            "description": "Minimal tool"
        }"#;

        let metadata: ToolMetadata = serde_json::from_str(json).unwrap();
        assert_eq!(metadata.name, "minimal");
        assert_eq!(metadata.version, ""); // default
        assert_eq!(metadata.wit_version, WIT_VERSION);
        assert!(!metadata.capabilities.has_http());
    }

    #[test]
    fn wit_version_validation() {
        // Matching version should succeed
        let metadata = ToolMetadata {
            name: "test".to_string(),
            description: "test".to_string(),
            version: "1.0.0".to_string(),
            wit_version: "0.1.0".to_string(),
            capabilities: Capabilities::default(),
            risk_level: None,
            author: None,
            homepage: None,
        };
        assert!(metadata.validate_wit_version().is_ok());

        // Mismatching version should fail
        let metadata = ToolMetadata {
            name: "test".to_string(),
            description: "test".to_string(),
            version: "1.0.0".to_string(),
            wit_version: "0.2.0".to_string(),
            capabilities: Capabilities::default(),
            risk_level: None,
            author: None,
            homepage: None,
        };
        assert!(metadata.validate_wit_version().is_err());
    }

    #[test]
    fn capabilities_path_generation() {
        let wasm_path = Path::new("/tools/echo.wasm");
        let caps_path = capabilities_path_for_wasm(wasm_path);
        assert_eq!(caps_path, PathBuf::from("/tools/echo.capabilities.json"));
    }

    #[test]
    fn risk_level_parsing() {
        let metadata: ToolMetadata = serde_json::from_str(
            r#"{
            "name": "test",
            "description": "test",
            "risk_level": "high"
        }"#,
        )
        .unwrap();

        assert_eq!(
            metadata.risk_level(),
            Some(crate::protocol::RiskLevel::High)
        );
    }

    #[test]
    fn from_file() {
        let temp_dir = TempDir::new().unwrap();
        let caps_path = temp_dir.path().join("test.capabilities.json");

        let mut file = std::fs::File::create(&caps_path).unwrap();
        file.write_all(br#"{"name": "test", "description": "A test tool"}"#)
            .unwrap();

        let metadata = ToolMetadata::from_file(&caps_path).unwrap();
        assert_eq!(metadata.name, "test");
        assert_eq!(metadata.description, "A test tool");
    }
}
