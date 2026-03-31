//! Write file tool implementation.
//!
//! This tool writes content to files with the following parameters:
//! - `path` (required): The file path to write
//! - `content` (required): The content to write
//!
//! # Security
//!
//! File writing has `RiskLevel::High` and includes:
//! - Maximum write size limit (5MB)
//! - Path traversal attack protection via `validate_path`
//! - Parent directory auto-creation

use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

use argus_protocol::llm::ToolDefinition;
use argus_protocol::risk_level::RiskLevel;
use argus_protocol::{NamedTool, ToolError, ToolExecutionContext};

use crate::path_utils::validate_path;

/// Maximum file size for writing (5MB).
const MAX_WRITE_SIZE: usize = 5 * 1024 * 1024;

/// Arguments for the write_file tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
#[allow(dead_code)]
struct WriteFileArgs {
    /// Path to the file to write
    path: String,
    /// Content to write to the file
    content: String,
}

/// Write file tool — writes content to files with path validation and size limit.
pub struct WriteFileTool;

impl Default for WriteFileTool {
    fn default() -> Self {
        Self::new()
    }
}

impl WriteFileTool {
    /// Create a new WriteFileTool.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl NamedTool for WriteFileTool {
    fn name(&self) -> &str {
        "write_file"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "write_file".to_string(),
            description:
                "Write content to a file on the filesystem. Creates the file if it doesn't \
                 exist, overwrites if it does. Parent directories are created automatically."
                    .to_string(),
            parameters: serde_json::to_value(schemars::schema_for!(WriteFileArgs))
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
                tool_name: "write_file".to_string(),
                reason: "Missing required parameter: path".to_string(),
            }
        })?;

        let content = input
            .get("content")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool_name: "write_file".to_string(),
                reason: "Missing required parameter: content".to_string(),
            })?;

        // Check content size
        if content.len() > MAX_WRITE_SIZE {
            return Err(ToolError::ExecutionFailed {
                tool_name: "write_file".to_string(),
                reason: format!(
                    "Content too large ({} bytes). Maximum is {} bytes.",
                    content.len(),
                    MAX_WRITE_SIZE
                ),
            });
        }

        // Validate path (sandboxing)
        let path = validate_path(path_str, None)?;

        // Create parent directories
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    tool_name: "write_file".to_string(),
                    reason: format!("Failed to create directories: {}", e),
                })?;
        }

        // Write file
        tokio::fs::write(&path, content)
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool_name: "write_file".to_string(),
                reason: format!("Failed to write file: {}", e),
            })?;

        Ok(json!({
            "path": path.to_string_lossy(),
            "bytes_written": content.len(),
            "success": true
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use argus_protocol::ids::ThreadId;
    use tokio::sync::broadcast;

    fn make_ctx() -> Arc<ToolExecutionContext> {
        let (tx, _) = broadcast::channel(16);
        let (control_tx, _control_rx) = tokio::sync::mpsc::unbounded_channel();
        Arc::new(ToolExecutionContext {
            thread_id: ThreadId::new(),
            agent_id: None,
            pipe_tx: tx,
            control_tx,
        })
    }

    #[test]
    fn test_write_file_name() {
        let tool = WriteFileTool::new();
        assert_eq!(tool.name(), "write_file");
    }

    #[test]
    fn test_write_file_risk_level() {
        let tool = WriteFileTool::new();
        assert_eq!(tool.risk_level(), RiskLevel::High);
    }

    #[tokio::test]
    async fn test_write_file_success() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("test.txt");

        let tool = WriteFileTool::new();
        let result = tool
            .execute(
                json!({
                    "path": path.to_str().unwrap(),
                    "content": "hello world"
                }),
                make_ctx(),
            )
            .await
            .unwrap();

        assert!(result["success"].as_bool().unwrap());
        assert_eq!(result["bytes_written"].as_i64().unwrap(), 11);

        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "hello world");
    }

    #[tokio::test]
    async fn test_write_file_creates_parents() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir
            .path()
            .join("subdir")
            .join("nested")
            .join("file.txt");

        let tool = WriteFileTool::new();
        let result = tool
            .execute(
                json!({
                    "path": path.to_str().unwrap(),
                    "content": "nested content"
                }),
                make_ctx(),
            )
            .await
            .unwrap();

        assert!(result["success"].as_bool().unwrap());
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "nested content");
    }

    #[tokio::test]
    async fn test_write_file_overwrite() {
        let temp_dir = tempfile::tempdir().unwrap();
        let path = temp_dir.path().join("overwrite.txt");
        std::fs::write(&path, "old content").unwrap();

        let tool = WriteFileTool::new();
        let result = tool
            .execute(
                json!({
                    "path": path.to_str().unwrap(),
                    "content": "new content"
                }),
                make_ctx(),
            )
            .await
            .unwrap();

        assert!(result["success"].as_bool().unwrap());
        let content = std::fs::read_to_string(&path).unwrap();
        assert_eq!(content, "new content");
    }

    #[tokio::test]
    async fn test_write_file_missing_path() {
        let tool = WriteFileTool::new();
        let result = tool.execute(json!({"content": "test"}), make_ctx()).await;
        assert!(result.is_err());
    }
}
