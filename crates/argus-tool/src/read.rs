//! Read tool implementation for reading file contents.
//!
//! This tool reads file contents asynchronously with the following parameters:
//! - `path` (required): The file path to read
//! - `offset` (optional): Line number to start reading from (1-indexed)
//! - `limit` (optional): Maximum number of lines to read
//!
//! # Security
//!
//! File reading is a sensitive operation. This tool has `RiskLevel::High`
//! and requires approval by default.

use async_trait::async_trait;
use serde_json::json;
use std::path::Path;
use std::sync::Arc;

use argus_protocol::llm::ToolDefinition;
use argus_protocol::risk_level::RiskLevel;
use argus_protocol::{NamedTool, ToolError, ToolExecutionContext};

/// Read tool implementation - reads file contents with risk level High.
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
            description: "Read file contents from the filesystem".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "path": {
                        "type": "string",
                        "description": "The file path to read"
                    },
                    "offset": {
                        "type": "number",
                        "description": "Line number to start reading from (1-indexed, default: 1)"
                    },
                    "limit": {
                        "type": "number",
                        "description": "Maximum number of lines to read (default: 2000)"
                    }
                },
                "required": ["path"]
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::High
    }

    async fn execute(&self, input: serde_json::Value, _ctx: Arc<ToolExecutionContext>) -> Result<serde_json::Value, ToolError> {
        // Parse path argument (required)
        let path = input.get("path").and_then(|v| v.as_str()).ok_or_else(|| {
            ToolError::ExecutionFailed {
                tool_name: "read".to_string(),
                reason: "Missing required parameter: path".to_string(),
            }
        })?;

        // Parse offset (optional, 1-indexed, default 1)
        let offset = input
            .get("offset")
            .and_then(|v| v.as_u64())
            .unwrap_or(1)
            .max(1) as usize;

        // Parse limit (optional, default 2000)
        let limit = input.get("limit").and_then(|v| v.as_u64()).unwrap_or(2000) as usize;

        let path = Path::new(path);

        // Check if path exists
        if !path.exists() {
            return Err(ToolError::ExecutionFailed {
                tool_name: "read".to_string(),
                reason: format!("Path does not exist: {}", path.display()),
            });
        }

        // Check if it's a file
        if !path.is_file() {
            return Err(ToolError::ExecutionFailed {
                tool_name: "read".to_string(),
                reason: format!("Path is not a file: {}", path.display()),
            });
        }

        // Read file content
        let content =
            tokio::fs::read_to_string(path)
                .await
                .map_err(|e| ToolError::ExecutionFailed {
                    tool_name: "read".to_string(),
                    reason: format!("Failed to read file: {}", e),
                })?;

        // Apply offset and limit
        let lines: Vec<&str> = content.lines().collect();
        let total_lines = lines.len();

        let start = (offset - 1).min(total_lines);
        let end = (start + limit).min(total_lines);

        let selected_lines = &lines[start..end];
        let result_content = selected_lines.join("\n");

        Ok(json!({
            "path": path.to_string_lossy(),
            "content": result_content,
            "total_lines": total_lines,
            "lines_shown": selected_lines.len(),
            "start_line": start + 1,
            "end_line": end
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
        Arc::new(ToolExecutionContext {
            thread_id: ThreadId::new(),
            pipe_tx: tx,
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
        assert!(result["content"].as_str().unwrap().contains("line 1"));
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
            .execute(json!({
                "path": file.path().to_str().unwrap(),
                "offset": 2,
                "limit": 2
            }), make_ctx())
            .await
            .unwrap();

        assert_eq!(result["start_line"], 2);
        assert_eq!(result["end_line"], 3);
        assert_eq!(result["lines_shown"], 2);
        let content = result["content"].as_str().unwrap();
        assert!(content.contains("line 2"));
        assert!(content.contains("line 3"));
        assert!(!content.contains("line 1"));
        assert!(!content.contains("line 4"));
    }

    #[tokio::test]
    async fn test_read_tool_file_not_found() {
        let tool = ReadTool::new();
        let result = tool.execute(json!({"path": "/nonexistent/path"}), make_ctx()).await;

        assert!(result.is_err());
        match result {
            Err(ToolError::ExecutionFailed { tool_name, reason }) => {
                assert_eq!(tool_name, "read");
                assert!(reason.contains("does not exist"));
            }
            _ => panic!("Expected ExecutionFailed error"),
        }
    }
}
