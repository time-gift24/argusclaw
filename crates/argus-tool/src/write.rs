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
use serde::Serialize;
use std::sync::Arc;

use argus_protocol::llm::ToolDefinition;
use argus_protocol::risk_level::RiskLevel;
use argus_protocol::{NamedTool, ToolError, ToolExecutionContext};

use crate::path_utils::{PathValidationError, validate_path};
use crate::{ToolOutputError, serialize_tool_output};

/// Maximum file size for writing (5MB).
const MAX_WRITE_SIZE: usize = 5 * 1024 * 1024;

/// Arguments for the write_file tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct WriteFileArgs {
    /// Path to the file to write
    path: String,
    /// Content to write to the file
    content: String,
}

#[derive(Debug, Serialize)]
struct WriteFileResponse {
    path: String,
    bytes_written: usize,
    success: bool,
}

#[derive(Debug, thiserror::Error)]
enum WriteFileToolError {
    #[error("Invalid arguments: {0}")]
    InvalidArguments(#[from] serde_json::Error),
    #[error("Content too large ({actual} bytes). Maximum is {max} bytes.")]
    ContentTooLarge { actual: usize, max: usize },
    #[error(transparent)]
    PathValidation(#[from] PathValidationError),
    #[error("Failed to create directories: {0}")]
    CreateDirectoriesFailed(std::io::Error),
    #[error("Failed to write file: {0}")]
    WriteFailed(std::io::Error),
    #[error(transparent)]
    Output(#[from] ToolOutputError),
}

impl From<WriteFileToolError> for ToolError {
    fn from(error: WriteFileToolError) -> Self {
        match error {
            WriteFileToolError::PathValidation(error) => error.into(),
            other => ToolError::ExecutionFailed {
                tool_name: WriteFileTool::TOOL_NAME.to_string(),
                reason: other.to_string(),
            },
        }
    }
}

/// Write file tool — writes content to files with path validation and size limit.
pub struct WriteFileTool;

impl Default for WriteFileTool {
    fn default() -> Self {
        Self::new()
    }
}

impl WriteFileTool {
    const TOOL_NAME: &'static str = "write_file";

    /// Create a new WriteFileTool.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    async fn execute_impl(
        &self,
        input: serde_json::Value,
    ) -> Result<WriteFileResponse, WriteFileToolError> {
        let args: WriteFileArgs = serde_json::from_value(input)?;

        // Check content size
        if args.content.len() > MAX_WRITE_SIZE {
            return Err(WriteFileToolError::ContentTooLarge {
                actual: args.content.len(),
                max: MAX_WRITE_SIZE,
            });
        }

        // Validate path (sandboxing)
        let path = validate_path(&args.path, None)?;

        // Create parent directories
        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(WriteFileToolError::CreateDirectoriesFailed)?;
        }

        // Write file
        tokio::fs::write(&path, &args.content)
            .await
            .map_err(WriteFileToolError::WriteFailed)?;

        Ok(WriteFileResponse {
            path: path.to_string_lossy().to_string(),
            bytes_written: args.content.len(),
            success: true,
        })
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
        let response = self.execute_impl(input).await.map_err(ToolError::from)?;
        serialize_tool_output(Self::TOOL_NAME, response)
            .map_err(WriteFileToolError::from)
            .map_err(ToolError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use argus_protocol::ids::ThreadId;
    use serde_json::json;
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
