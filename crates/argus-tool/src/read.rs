//! Read tool implementation for reading file contents.
//!
//! This tool reads file contents asynchronously with the following parameters:
//! - `path` (required): The file path to read
//! - `offset` (optional): Line number to start reading from (1-indexed)
//! - `limit` (optional): Maximum number of lines to read
//!
//! # Security
//!
//! File reading has `RiskLevel::High` and includes:
//! - Maximum file size limit (1MB)
//! - Path traversal attack protection via `validate_path`
//! - Null byte and URL-encoded separator detection

use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;

use argus_protocol::llm::ToolDefinition;
use argus_protocol::risk_level::RiskLevel;
use argus_protocol::{NamedTool, ToolError, ToolExecutionContext};

use crate::path_utils::validate_path;

/// Maximum file size for reading (1MB).
const MAX_READ_SIZE: u64 = 1024 * 1024;

/// Read tool implementation — reads file contents with path validation and size limit.
pub struct ReadTool;

impl Default for ReadTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ReadTool {
    /// Create a new ReadTool.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

#[async_trait]
impl NamedTool for ReadTool {
    fn name(&self) -> &str {
        "read"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "read".to_string(),
            description:
                "Read a file from the filesystem. Returns file content as text with line numbers. \
                 For large files, specify offset and limit to read a portion."
                    .to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "Path to the file to read"
                    },
                    "offset": {
                        "type": "number",
                        "description": "Line number to start reading from (1-indexed, optional, default: 1)"
                    },
                    "limit": {
                        "type": "number",
                        "description": "Maximum number of lines to read (optional)"
                    }
                },
                "required": ["path"]
            }),
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
                tool_name: "read".to_string(),
                reason: "Missing required parameter: path".to_string(),
            }
        })?;

        let offset = input
            .get("offset")
            .and_then(|v| v.as_u64())
            .unwrap_or(1)
            .max(1) as usize;
        let limit = input.get("limit").and_then(|v| v.as_u64());

        // Validate path (sandboxing)
        let path = validate_path(path_str, None)?;

        // Check file exists and is a file
        if !path.exists() {
            return Err(ToolError::ExecutionFailed {
                tool_name: "read".to_string(),
                reason: format!("Path does not exist: {}", path.display()),
            });
        }
        if !path.is_file() {
            return Err(ToolError::ExecutionFailed {
                tool_name: "read".to_string(),
                reason: format!("Path is not a file: {}", path.display()),
            });
        }

        // Check file size
        let metadata =
            tokio::fs::metadata(&path)
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    tool_name: "read".to_string(),
                    reason: format!("Cannot access file: {}", e),
                })?;

        if metadata.len() > MAX_READ_SIZE {
            return Err(ToolError::ExecutionFailed {
                tool_name: "read".to_string(),
                reason: format!(
                    "File too large ({} bytes). Maximum is {} bytes. Use offset/limit for partial reads.",
                    metadata.len(),
                    MAX_READ_SIZE
                ),
            });
        }

        // Read file
        let content =
            tokio::fs::read_to_string(&path)
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    tool_name: "read".to_string(),
                    reason: format!("Failed to read file: {}", e),
                })?;

        // Apply offset and limit
        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        let start_line = offset.saturating_sub(1).min(total_lines);
        let end_line = if let Some(lim) = limit {
            (start_line + lim as usize).min(total_lines)
        } else {
            total_lines
        };

        // Format with line numbers (ironclaw style)
        let selected_lines: Vec<String> = lines[start_line..end_line]
            .iter()
            .enumerate()
            .map(|(i, line)| format!("{:>6}│ {}", start_line + i + 1, line))
            .collect();

        Ok(json!({
            "content": selected_lines.join("\n"),
            "total_lines": total_lines,
            "lines_shown": end_line - start_line,
            "path": path.to_string_lossy()
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
            agent_id: None,
            pipe_tx: tx,
            control_tx,
        })
    }

    #[test]
    fn test_read_tool_name() {
        let tool = ReadTool::new();
        assert_eq!(tool.name(), "read");
    }

    #[test]
    fn test_read_tool_risk_level() {
        let tool = ReadTool::new();
        assert_eq!(tool.risk_level(), RiskLevel::High);
    }

    #[test]
    fn test_read_tool_definition() {
        let tool = ReadTool::new();
        let def = tool.definition();
        assert_eq!(def.name, "read");
        assert!(
            def.parameters["required"]
                .as_array()
                .unwrap()
                .contains(&json!("path"))
        );
    }

    #[tokio::test]
    async fn test_read_tool_success() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "line 1").unwrap();
        writeln!(file, "line 2").unwrap();
        writeln!(file, "line 3").unwrap();

        let tool = ReadTool::new();
        let result = tool
            .execute(json!({"path": file.path().to_str().unwrap()}), make_ctx())
            .await
            .unwrap();

        assert_eq!(result["total_lines"], 3);
        let content = result["content"].as_str().unwrap();
        assert!(content.contains("line 1"));
        assert!(content.contains("line 2"));
        // Ironclaw-style: line numbers included
        assert!(content.contains("     1│"));
        assert!(content.contains("     2│"));
    }

    #[tokio::test]
    async fn test_read_tool_with_offset_and_limit() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "line 1").unwrap();
        writeln!(file, "line 2").unwrap();
        writeln!(file, "line 3").unwrap();
        writeln!(file, "line 4").unwrap();
        writeln!(file, "line 5").unwrap();

        let tool = ReadTool::new();
        let result = tool
            .execute(
                json!({
                    "path": file.path().to_str().unwrap(),
                    "offset": 2,
                    "limit": 2
                }),
                make_ctx(),
            )
            .await
            .unwrap();

        assert_eq!(result["lines_shown"], 2);
        let content = result["content"].as_str().unwrap();
        // Line numbers should match 2 and 3
        assert!(content.contains("     2│"));
        assert!(content.contains("     3│"));
        assert!(!content.contains("     1│"));
        assert!(!content.contains("     4│"));
    }

    #[tokio::test]
    async fn test_read_tool_file_not_found() {
        let tool = ReadTool::new();
        let result = tool
            .execute(json!({"path": "/nonexistent/path"}), make_ctx())
            .await;

        assert!(result.is_err());
        match result {
            Err(ToolError::ExecutionFailed { tool_name, reason }) => {
                assert_eq!(tool_name, "read");
                assert!(reason.contains("does not exist"));
            }
            _ => panic!("Expected ExecutionFailed error"),
        }
    }

    #[tokio::test]
    async fn test_read_tool_file_size_limit() {
        let mut file = NamedTempFile::new().unwrap();
        // Write more than 1MB
        let data = vec![b'x'; 1024 * 1024 + 1];
        file.write_all(&data).unwrap();

        let tool = ReadTool::new();
        let result = tool
            .execute(json!({"path": file.path().to_str().unwrap()}), make_ctx())
            .await;

        assert!(result.is_err());
        match result {
            Err(ToolError::ExecutionFailed { reason, .. }) => {
                assert!(reason.contains("too large"));
            }
            _ => panic!("Expected ExecutionFailed for oversized file"),
        }
    }
}
