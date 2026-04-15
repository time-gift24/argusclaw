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
use serde::Serialize;
use std::sync::Arc;

use argus_protocol::llm::ToolDefinition;
use argus_protocol::risk_level::RiskLevel;
use argus_protocol::{NamedTool, ToolError, ToolExecutionContext};

use crate::path_utils::{PathValidationError, validate_path};
use crate::{ToolOutputError, serialize_tool_output};

/// Arguments for the apply_patch tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
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

#[derive(Debug, Serialize)]
struct ApplyPatchResponse {
    path: String,
    replacements: usize,
    success: bool,
}

#[derive(Debug, thiserror::Error)]
enum ApplyPatchToolError {
    #[error("Invalid arguments: {0}")]
    InvalidArguments(#[from] serde_json::Error),
    #[error(transparent)]
    PathValidation(#[from] PathValidationError),
    #[error("Failed to read file: {0}")]
    ReadFailed(std::io::Error),
    #[error("Could not find the specified text in {path}. Make sure old_string matches exactly.")]
    OldStringNotFound { path: String },
    #[error("Failed to write file: {0}")]
    WriteFailed(std::io::Error),
    #[error(transparent)]
    Output(#[from] ToolOutputError),
}

impl From<ApplyPatchToolError> for ToolError {
    fn from(error: ApplyPatchToolError) -> Self {
        match error {
            ApplyPatchToolError::PathValidation(error) => error.into(),
            other => ToolError::ExecutionFailed {
                tool_name: ApplyPatchTool::TOOL_NAME.to_string(),
                reason: other.to_string(),
            },
        }
    }
}

/// Apply patch tool — applies search/replace edits to files.
pub struct ApplyPatchTool;

impl Default for ApplyPatchTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ApplyPatchTool {
    const TOOL_NAME: &'static str = "apply_patch";

    /// Create a new ApplyPatchTool.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    async fn execute_impl(
        &self,
        input: serde_json::Value,
    ) -> Result<ApplyPatchResponse, ApplyPatchToolError> {
        let args: ApplyPatchArgs = serde_json::from_value(input)?;
        let replace_all = args.replace_all.unwrap_or(false);

        // Validate path (sandboxing)
        let path = validate_path(&args.path, None)?;

        // Read current content
        let content = tokio::fs::read_to_string(&path)
            .await
            .map_err(ApplyPatchToolError::ReadFailed)?;

        // Check if old_string exists
        if !content.contains(&args.old_string) {
            return Err(ApplyPatchToolError::OldStringNotFound {
                path: path.display().to_string(),
            });
        }

        // Apply replacement
        let new_content = if replace_all {
            content.replace(&args.old_string, &args.new_string)
        } else {
            content.replacen(&args.old_string, &args.new_string, 1)
        };

        // Count replacements
        let replacements = if replace_all {
            content.matches(&args.old_string).count()
        } else {
            1
        };

        // Write back
        tokio::fs::write(&path, &new_content)
            .await
            .map_err(ApplyPatchToolError::WriteFailed)?;

        Ok(ApplyPatchResponse {
            path: path.to_string_lossy().to_string(),
            replacements,
            success: true,
        })
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
        let response = self.execute_impl(input).await.map_err(ToolError::from)?;
        serialize_tool_output(Self::TOOL_NAME, response)
            .map_err(ApplyPatchToolError::from)
            .map_err(ToolError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use argus_protocol::ids::ThreadId;
    use serde_json::json;
    use std::io::Write;
    use tempfile::NamedTempFile;
    use tokio::sync::broadcast;

    fn make_ctx() -> Arc<ToolExecutionContext> {
        let (tx, _) = broadcast::channel(16);
        Arc::new(ToolExecutionContext {
            thread_id: ThreadId::new(),
            agent_id: None,
            pipe_tx: tx,
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
