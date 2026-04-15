//! Glob tool implementation for file pattern matching.
//!
//! This tool finds files matching glob patterns asynchronously with the following parameters:
//! - `pattern` (required): The glob pattern to match files
//! - `path` (optional): Directory to search in (default: current directory)
//!
//! # Security
//!
//! File system traversal is a sensitive operation. This tool has `RiskLevel::High`
//! so callers can apply appropriate policy or UI treatment.

use async_trait::async_trait;
use serde::Serialize;
use std::sync::Arc;

use argus_protocol::llm::ToolDefinition;
use argus_protocol::risk_level::RiskLevel;
use argus_protocol::{NamedTool, ToolError, ToolExecutionContext};

use crate::{ToolOutputError, serialize_tool_output};

/// Maximum number of results to return.
const MAX_RESULTS: usize = 1000;

/// Arguments for the glob tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct GlobArgs {
    /// The glob pattern to match files (e.g., "**/*.rs", "src/**/*.ts")
    pattern: String,
    /// Directory to search in (default: current directory)
    #[serde(default)]
    path: Option<String>,
}

#[derive(Debug, Serialize)]
struct GlobResponse {
    pattern: String,
    path: String,
    count: usize,
    truncated: bool,
    files: Vec<String>,
}

#[derive(Debug, thiserror::Error)]
enum GlobToolError {
    #[error("Invalid arguments: {0}")]
    InvalidArguments(#[from] serde_json::Error),
    #[error("Path does not exist: {0}")]
    PathDoesNotExist(String),
    #[error("Invalid glob pattern: {0}")]
    InvalidPattern(glob::PatternError),
    #[error(transparent)]
    Output(#[from] ToolOutputError),
}

impl From<GlobToolError> for ToolError {
    fn from(error: GlobToolError) -> Self {
        ToolError::ExecutionFailed {
            tool_name: GlobTool::TOOL_NAME.to_string(),
            reason: error.to_string(),
        }
    }
}

/// Glob tool implementation - finds files matching patterns with risk level High.
pub struct GlobTool;

impl Default for GlobTool {
    fn default() -> Self {
        Self::new()
    }
}

impl GlobTool {
    const TOOL_NAME: &'static str = "glob";

    /// Create a new GlobTool.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    async fn execute_impl(&self, input: serde_json::Value) -> Result<GlobResponse, GlobToolError> {
        let args: GlobArgs = serde_json::from_value(input)?;

        // Parse path (optional, default current directory)
        let base_path = args
            .path
            .as_deref()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        // Validate base path exists
        if !base_path.exists() {
            return Err(GlobToolError::PathDoesNotExist(
                base_path.display().to_string(),
            ));
        }

        // Build full glob pattern
        let full_pattern = if base_path.is_dir() {
            format!("{}/{}", base_path.display(), args.pattern)
        } else {
            args.pattern.clone()
        };

        // Execute glob
        let mut files = Vec::new();
        let glob = glob::glob(&full_pattern).map_err(GlobToolError::InvalidPattern)?;

        for entry in glob {
            match entry {
                Ok(path) => {
                    files.push(path.to_string_lossy().to_string());
                }
                Err(e) => {
                    tracing::debug!("Skipping path due to error: {}", e);
                }
            }

            if files.len() >= MAX_RESULTS {
                break;
            }
        }

        let truncated = files.len() >= MAX_RESULTS;
        if truncated {
            files.truncate(MAX_RESULTS);
        }

        Ok(GlobResponse {
            pattern: args.pattern,
            path: base_path.to_string_lossy().to_string(),
            count: files.len(),
            truncated,
            files,
        })
    }
}

#[async_trait]
impl NamedTool for GlobTool {
    fn name(&self) -> &str {
        "glob"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "glob".to_string(),
            description: "Find files matching glob patterns (e.g., \"**/*.rs\", \"src/**/*.ts\")"
                .to_string(),
            parameters: serde_json::to_value(schemars::schema_for!(GlobArgs))
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
            .map_err(GlobToolError::from)
            .map_err(ToolError::from)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use argus_protocol::ids::ThreadId;
    use serde_json::json;
    use std::fs;
    use tempfile::tempdir;
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
    fn test_glob_tool_name() {
        let tool = GlobTool::new();
        assert_eq!(tool.name(), "glob");
    }

    #[test]
    fn test_glob_tool_risk_level() {
        let tool = GlobTool::new();
        assert_eq!(tool.risk_level(), RiskLevel::High);
    }

    #[test]
    fn test_glob_tool_definition() {
        let tool = GlobTool::new();
        let def = tool.definition();
        assert_eq!(def.name, "glob");
        assert!(
            def.parameters["required"]
                .as_array()
                .unwrap()
                .contains(&json!("pattern"))
        );
    }

    #[tokio::test]
    async fn test_glob_tool_find_files() {
        let dir = tempdir().unwrap();
        fs::File::create(dir.path().join("test.rs")).unwrap();
        fs::File::create(dir.path().join("test.txt")).unwrap();
        fs::File::create(dir.path().join("other.rs")).unwrap();

        let tool = GlobTool::new();
        let result = tool
            .execute(
                json!({
                    "pattern": "*.rs",
                    "path": dir.path().to_str().unwrap()
                }),
                make_ctx(),
            )
            .await
            .unwrap();

        assert_eq!(result["count"], 2);
        let files = result["files"].as_array().unwrap();
        assert!(
            files
                .iter()
                .any(|f| f.as_str().unwrap().ends_with("test.rs"))
        );
        assert!(
            files
                .iter()
                .any(|f| f.as_str().unwrap().ends_with("other.rs"))
        );
    }

    #[tokio::test]
    async fn test_glob_tool_recursive() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::File::create(dir.path().join("src").join("main.rs")).unwrap();
        fs::File::create(dir.path().join("src").join("lib.rs")).unwrap();

        let tool = GlobTool::new();
        let result = tool
            .execute(
                json!({
                    "pattern": "**/*.rs",
                    "path": dir.path().to_str().unwrap()
                }),
                make_ctx(),
            )
            .await
            .unwrap();

        assert_eq!(result["count"], 2);
    }

    #[tokio::test]
    async fn test_glob_tool_no_matches() {
        let dir = tempdir().unwrap();

        let tool = GlobTool::new();
        let result = tool
            .execute(
                json!({
                    "pattern": "*.nonexistent",
                    "path": dir.path().to_str().unwrap()
                }),
                make_ctx(),
            )
            .await
            .unwrap();

        assert_eq!(result["count"], 0);
    }

    #[tokio::test]
    async fn test_glob_tool_invalid_pattern() {
        let tool = GlobTool::new();
        let result = tool
            .execute(
                json!({
                    "pattern": "[invalid",
                    "path": "."
                }),
                make_ctx(),
            )
            .await;

        assert!(result.is_err());
        match result {
            Err(ToolError::ExecutionFailed { tool_name, reason }) => {
                assert_eq!(tool_name, "glob");
                assert!(reason.contains("Invalid glob pattern"));
            }
            _ => panic!("Expected ExecutionFailed error"),
        }
    }
}
