//! Grep tool implementation for searching file contents.
//!
//! This tool searches for patterns in files asynchronously with the following parameters:
//! - `pattern` (required): The regex pattern to search for
//! - `path` (optional): Directory or file path to search in (default: current directory)
//! - `glob` (optional): Glob pattern to filter files (default: "*")
//! - `ignore_case` (optional): Case insensitive search (default: false)
//!
//! # Security
//!
//! File content searching is a sensitive operation. This tool has `RiskLevel::High`
//! so callers can apply appropriate policy or UI treatment.

use async_trait::async_trait;
use regex::RegexBuilder;
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;

use argus_protocol::llm::ToolDefinition;
use argus_protocol::risk_level::RiskLevel;
use argus_protocol::{NamedTool, ToolError, ToolExecutionContext};

use crate::{ToolOutputError, serialize_tool_output};

/// Maximum number of matches to return.
const MAX_MATCHES: usize = 100;

/// Arguments for the grep tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct GrepArgs {
    /// The regex pattern to search for
    pattern: String,
    /// Directory or file path to search in (default: current directory)
    #[serde(default)]
    path: Option<String>,
    /// Glob pattern to filter files (default: "*")
    #[serde(default, rename = "glob")]
    glob_pattern: Option<String>,
    /// Case insensitive search (default: false)
    #[serde(default)]
    ignore_case: Option<bool>,
}

#[derive(Debug, Serialize)]
struct GrepMatch {
    file: String,
    line: usize,
    content: String,
}

#[derive(Debug, Serialize)]
struct GrepResponse {
    pattern: String,
    path: String,
    files_searched: usize,
    matches_count: usize,
    truncated: bool,
    matches: Vec<GrepMatch>,
}

#[derive(Debug, thiserror::Error)]
enum GrepToolError {
    #[error("Invalid arguments: {0}")]
    InvalidArguments(#[from] serde_json::Error),
    #[error("Invalid regex pattern: {0}")]
    InvalidRegex(regex::Error),
    #[error("Path does not exist: {0}")]
    PathDoesNotExist(String),
    #[error("Invalid glob pattern: {0}")]
    InvalidGlob(glob::PatternError),
    #[error(transparent)]
    Output(#[from] ToolOutputError),
}

impl From<GrepToolError> for ToolError {
    fn from(error: GrepToolError) -> Self {
        ToolError::ExecutionFailed {
            tool_name: GrepTool::TOOL_NAME.to_string(),
            reason: error.to_string(),
        }
    }
}

/// Grep tool implementation - searches file contents with risk level High.
pub struct GrepTool;

impl Default for GrepTool {
    fn default() -> Self {
        Self::new()
    }
}

impl GrepTool {
    const TOOL_NAME: &'static str = "grep";

    /// Create a new GrepTool.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    async fn execute_impl(&self, input: serde_json::Value) -> Result<GrepResponse, GrepToolError> {
        let args: GrepArgs = serde_json::from_value(input)?;

        // Parse path (optional, default current directory)
        let path = args
            .path
            .as_deref()
            .map(std::path::PathBuf::from)
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

        // Parse glob pattern (optional, default "*")
        let glob_pattern = args.glob_pattern.as_deref().unwrap_or("*");

        // Parse ignore_case (optional, default false)
        let ignore_case = args.ignore_case.unwrap_or(false);

        // Build regex
        let re = RegexBuilder::new(&args.pattern)
            .case_insensitive(ignore_case)
            .build()
            .map_err(GrepToolError::InvalidRegex)?;

        // Search files
        let mut matches = Vec::new();
        let mut files_searched = 0;

        self.search_path(&path, &re, glob_pattern, &mut matches, &mut files_searched)
            .await?;

        // Truncate if too many matches
        let truncated = matches.len() > MAX_MATCHES;
        if truncated {
            matches.truncate(MAX_MATCHES);
        }

        Ok(GrepResponse {
            pattern: args.pattern,
            path: path.to_string_lossy().to_string(),
            files_searched,
            matches_count: matches.len(),
            truncated,
            matches,
        })
    }
}

#[async_trait]
impl NamedTool for GrepTool {
    fn name(&self) -> &str {
        "grep"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "grep".to_string(),
            description: "Search for patterns in files using regex".to_string(),
            parameters: serde_json::to_value(schemars::schema_for!(GrepArgs))
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
            .map_err(GrepToolError::from)
            .map_err(ToolError::from)
    }
}

impl GrepTool {
    async fn search_path(
        &self,
        path: &Path,
        re: &regex::Regex,
        glob_pattern: &str,
        matches: &mut Vec<GrepMatch>,
        files_searched: &mut usize,
    ) -> Result<(), GrepToolError> {
        if !path.exists() {
            return Err(GrepToolError::PathDoesNotExist(path.display().to_string()));
        }

        if path.is_file() {
            self.search_file(path, re, matches).await;
            *files_searched += 1;
        } else if path.is_dir() {
            let glob = glob::glob(&format!("{}/{}", path.display(), glob_pattern))
                .map_err(GrepToolError::InvalidGlob)?;

            for entry in glob {
                match entry {
                    Ok(entry_path) => {
                        if entry_path.is_file() {
                            self.search_file(&entry_path, re, matches).await;
                            *files_searched += 1;
                        }
                    }
                    Err(e) => {
                        // Skip files we can't access
                        tracing::debug!("Skipping file due to error: {}", e);
                    }
                }

                // Stop if we have enough matches
                if matches.len() >= MAX_MATCHES {
                    break;
                }
            }
        }

        Ok(())
    }

    async fn search_file(&self, path: &Path, re: &regex::Regex, matches: &mut Vec<GrepMatch>) {
        let content = match tokio::fs::read_to_string(path).await {
            Ok(c) => c,
            Err(_) => return, // Skip files we can't read
        };

        for (line_num, line) in content.lines().enumerate() {
            if re.is_match(line) {
                matches.push(GrepMatch {
                    file: path.to_string_lossy().to_string(),
                    line: line_num + 1,
                    content: line.to_string(),
                });

                if matches.len() >= MAX_MATCHES {
                    return;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use argus_protocol::ids::ThreadId;
    use serde_json::json;
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
    fn test_grep_tool_name() {
        let tool = GrepTool::new();
        assert_eq!(tool.name(), "grep");
    }

    #[test]
    fn test_grep_tool_risk_level() {
        let tool = GrepTool::new();
        assert_eq!(tool.risk_level(), RiskLevel::High);
    }

    #[test]
    fn test_grep_tool_definition() {
        let tool = GrepTool::new();
        let def = tool.definition();
        assert_eq!(def.name, "grep");
        assert!(
            def.parameters["required"]
                .as_array()
                .unwrap()
                .contains(&json!("pattern"))
        );
    }

    #[tokio::test]
    async fn test_grep_tool_single_file() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        tokio::fs::write(&file_path, "hello world\nfoo bar\nhello again\n")
            .await
            .unwrap();

        let tool = GrepTool::new();
        let result = tool
            .execute(
                json!({
                    "pattern": "hello",
                    "path": file_path.to_str().unwrap()
                }),
                make_ctx(),
            )
            .await
            .unwrap();

        assert_eq!(result["matches_count"], 2);
        let matches = result["matches"].as_array().unwrap();
        assert!(matches[0]["content"].as_str().unwrap().contains("hello"));
    }

    #[tokio::test]
    async fn test_grep_tool_ignore_case() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        tokio::fs::write(&file_path, "HELLO world\nfoo bar\n")
            .await
            .unwrap();

        let tool = GrepTool::new();
        let result = tool
            .execute(
                json!({
                    "pattern": "hello",
                    "path": file_path.to_str().unwrap(),
                    "ignore_case": true
                }),
                make_ctx(),
            )
            .await
            .unwrap();

        assert_eq!(result["matches_count"], 1);
    }

    #[tokio::test]
    async fn test_grep_tool_no_matches() {
        let dir = tempdir().unwrap();
        let file_path = dir.path().join("test.txt");
        tokio::fs::write(&file_path, "hello world\nfoo bar\n")
            .await
            .unwrap();

        let tool = GrepTool::new();
        let result = tool
            .execute(
                json!({
                    "pattern": "nonexistent",
                    "path": file_path.to_str().unwrap()
                }),
                make_ctx(),
            )
            .await
            .unwrap();

        assert_eq!(result["matches_count"], 0);
    }

    #[tokio::test]
    async fn test_grep_tool_invalid_regex() {
        let tool = GrepTool::new();
        let result = tool
            .execute(json!({"pattern": "[invalid"}), make_ctx())
            .await;

        assert!(result.is_err());
        match result {
            Err(ToolError::ExecutionFailed { tool_name, reason }) => {
                assert_eq!(tool_name, "grep");
                assert!(reason.contains("Invalid regex pattern"));
            }
            _ => panic!("Expected ExecutionFailed error"),
        }
    }
}
