//! Apply patch tool for targeted file edits.
//!
//! This tool applies search/replace edits to files with the following parameters:
//! - `path` (required): The file to edit
//! - `old_string` (required): The exact string to find
//! - `new_string` (required): The replacement string
//! - `replace_all` (optional): Replace all occurrences (default false)
//!
//! # Security
//!
//! Patch has `RiskLevel::High` and uses `validate_path` for sandboxing.

use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

use argus_protocol::llm::ToolDefinition;
use argus_protocol::risk_level::RiskLevel;
use argus_protocol::{NamedTool, ToolError, ToolExecutionContext};

use crate::path_utils::validate_path;

/// Arguments for the apply_patch tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[allow(dead_code)]
struct ApplyPatchArgs {
    /// Path to the file to edit
    path: String,
    /// The exact string to find and replace
    old_string: String,
    /// The string to replace it with
    new_string: String,
    /// If true, replace all occurrences (default false)
    #[serde(default)]
    replace_all: Option<bool>,
}

/// Apply patch tool — applies search/replace edits to files.
pub struct ApplyPatchTool;

impl Default for ApplyPatchTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ApplyPatchTool {
    /// Create a new ApplyPatchTool.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl NamedTool for ApplyPatchTool {
    fn name(&self) -> &str {
        "apply_patch"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "apply_patch".to_string(),
            description: "Apply targeted edits to a file using search/replace. Finds the exact \
                 'old_string' and replaces it with 'new_string'. Use for surgical changes."
                .to_string(),
            parameters: serde_json::to_value(schemars::schema_for!(ApplyPatchArgs))
                .unwrap_or_else(|_| serde_json::json!({"type": "object"})),
        }
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::High
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: Arc<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError> {
        let path_str = input.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
            ToolError::ExecutionFailed {
                tool_name: "apply_patch".to_string(),
                reason: "Missing required parameter: path".to_string(),
            }
        })?;

        let old_string = input
            .get("old_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool_name: "apply_patch".to_string(),
                reason: "Missing required parameter: old_string".to_string(),
            })?;

        let new_string = input
            .get("new_string")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool_name: "apply_patch".to_string(),
                reason: "Missing required parameter: new_string".to_string(),
            })?;

        let replace_all = input
            .get("replace_all")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        // Validate path (sandboxing)
        let path = validate_path(path_str, None)?;

        // Read current content
        let content =
            tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    tool_name: "apply_patch".to_string(),
                    reason: format!("Failed to read file: {}", e),
                })?;

        // Check if old_string exists
        if !content.contains(old_string) {
            return Err(ToolError::ExecutionFailed {
                tool_name: "apply_patch".to_string(),
                reason: format!(
                    "Could not find the specified text in {}. Make sure old_string matches exactly.",
                    path.display()
                ),
            });
        }

        // Apply replacement
        let new_content = if replace_all {
            content.replace(old_string, new_string)
        } else {
            content.replacen(old_string, new_string, 1)
        };

        // Count replacements
        let replacements = if replace_all {
            content.matches(old_string).count()
        } else {
            1
        };

        // Write back
        tokio::fs::write(&path, &new_content)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool_name: "apply_patch".to_string(),
                reason: format!("Failed to write file: {}", e),
            })?;

        Ok(json!({
            "path": path.to_string_lossy(),
            "replacements": replacements,
            "success": true
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use argus_protocol::ids::ThreadId;
    use std::io::Write;
    use tempfile::NamedTempFile;
    use tokio::sync::broadcast;

    fn make_ctx() -> Arc<ToolExecutionContext> {
        let (tx, _) = broadcast::channel(16);
        let (control_tx, _control_rx) = tokio::sync::mpsc::unbounded_channel();
        Arc::new(ToolExecutionContext {
            thread_id: ThreadId::new(),
            pipe_tx: tx,
            control_tx,
        })
    }

    #[test]
    fn test_apply_patch_name() {
        let tool = ApplyPatchTool::new();
        assert_eq!(tool.name(), "apply_patch");
    }

    #[test]
    fn test_apply_patch_risk_level() {
        let tool = ApplyPatchTool::new();
        assert_eq!(tool.risk_level(), RiskLevel::High);
    }

    #[tokio::test]
    async fn test_apply_patch_success() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "fn main() {{").unwrap();
        writeln!(file, "    println!(\"old\");").unwrap();
        writeln!(file, "}}").unwrap();

        let tool = ApplyPatchTool::new();
        let result = tool
            .execute(
                json!({
                    "path": file.path().to_str().unwrap(),
                    "old_string": "println!(\"old\")",
                    "new_string": "println!(\"new\")"
                }),
                make_ctx(),
            )
            .await
            .unwrap();

        assert!(result["success"].as_bool().unwrap());
        assert_eq!(result["replacements"].as_i64().unwrap(), 1);

        let content = std::fs::read_to_string(file.path()).unwrap();
        assert!(content.contains("println!(\"new\")"));
        assert!(!content.contains("println!(\"old\")"));
    }

    #[tokio::test]
    async fn test_apply_patch_replace_all() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "let x = foo;").unwrap();
        writeln!(file, "let y = foo;").unwrap();

        let tool = ApplyPatchTool::new();
        let result = tool
            .execute(
                json!({
                    "path": file.path().to_str().unwrap(),
                    "old_string": "foo",
                    "new_string": "bar",
                    "replace_all": true
                }),
                make_ctx(),
            )
            .await
            .unwrap();

        assert!(result["success"].as_bool().unwrap());
        assert_eq!(result["replacements"].as_i64().unwrap(), 2);

        let content = std::fs::read_to_string(file.path()).unwrap();
        assert!(content.contains("let x = bar;"));
        assert!(content.contains("let y = bar;"));
    }

    #[tokio::test]
    async fn test_apply_patch_not_found() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "fn main() {{}}").unwrap();

        let tool = ApplyPatchTool::new();
        let result = tool
            .execute(
                json!({
                    "path": file.path().to_str().unwrap(),
                    "old_string": "nonexistent",
                    "new_string": "replacement"
                }),
                make_ctx(),
            )
            .await;

        assert!(result.is_err());
        match result {
            Err(ToolError::ExecutionFailed { reason, .. }) => {
                assert!(reason.contains("Could not find"));
            }
            _ => panic!("Expected ExecutionFailed"),
        }
    }
}
