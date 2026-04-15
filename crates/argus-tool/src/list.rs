//! List directory tool implementation.
//!
//! This tool lists directory contents with the following parameters:
//! - `path` (optional): Directory to list (defaults to current directory)
//! - `recursive` (optional): Whether to list recursively (default false)
//! - `max_depth` (optional): Maximum depth for recursive listing (default 3)
//!
//! # Security
//!
//! Directory listing has `RiskLevel::Medium` and skips common non-essential directories.

use async_trait::async_trait;
use serde::Serialize;
use std::path::Path;
use std::sync::Arc;

use argus_protocol::llm::ToolDefinition;
use argus_protocol::risk_level::RiskLevel;
use argus_protocol::{NamedTool, ToolError, ToolExecutionContext};

use crate::path_utils::{PathValidationError, validate_path};
use crate::{ToolOutputError, serialize_tool_output};

/// Maximum directory listing entries.
const MAX_DIR_ENTRIES: usize = 500;

/// Arguments for the list_dir tool.
#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
struct ListDirArgs {
    /// Path to the directory to list (defaults to current directory)
    #[serde(default)]
    path: Option<String>,
    /// If true, list contents recursively (default false)
    #[serde(default)]
    recursive: Option<bool>,
    /// Maximum depth for recursive listing (default 3)
    #[serde(default)]
    max_depth: Option<usize>,
}

#[derive(Debug, Serialize)]
struct ListDirResponse {
    path: String,
    entries: Vec<String>,
    count: usize,
    truncated: bool,
}

#[derive(Debug, thiserror::Error)]
enum ListDirToolError {
    #[error("Invalid arguments: {0}")]
    InvalidArguments(#[from] serde_json::Error),
    #[error(transparent)]
    PathValidation(#[from] PathValidationError),
    #[error("Path is not a directory: {0}")]
    PathIsNotDirectory(String),
    #[error("Failed to read directory: {0}")]
    ReadDirectoryFailed(std::io::Error),
    #[error("Failed to read entry: {0}")]
    ReadEntryFailed(std::io::Error),
    #[error(transparent)]
    Output(#[from] ToolOutputError),
}

impl From<ListDirToolError> for ToolError {
    fn from(error: ListDirToolError) -> Self {
        match error {
            ListDirToolError::PathValidation(error) => error.into(),
            other => ToolError::ExecutionFailed {
                tool_name: ListDirTool::TOOL_NAME.to_string(),
                reason: other.to_string(),
            },
        }
    }
}

/// Directories to skip during recursive listing.
const SKIP_DIRS: &[&str] = &[
    "node_modules",
    "target",
    ".git",
    "__pycache__",
    "venv",
    ".venv",
];

/// List directory tool — lists directory contents with optional recursion.
pub struct ListDirTool;

impl Default for ListDirTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ListDirTool {
    const TOOL_NAME: &'static str = "list_dir";

    /// Create a new ListDirTool.
    #[must_use]
    pub fn new() -> Self {
        Self
    }

    async fn execute_impl(
        &self,
        input: serde_json::Value,
    ) -> Result<ListDirResponse, ListDirToolError> {
        let args: ListDirArgs = serde_json::from_value(input)?;
        let path_str = args.path.as_deref().unwrap_or(".");
        let recursive = args.recursive.unwrap_or(false);
        let max_depth = args.max_depth.unwrap_or(3);

        // Validate path (sandboxing)
        let path = validate_path(path_str, None)?;

        if !path.is_dir() {
            return Err(ListDirToolError::PathIsNotDirectory(
                path.display().to_string(),
            ));
        }

        let mut entries = Vec::new();
        list_dir_inner(&path, &path, recursive, max_depth, 0, &mut entries).await?;

        // Sort entries: directories first, then alphabetically
        entries.sort_by(|a, b| {
            let a_is_dir = a.ends_with('/');
            let b_is_dir = b.ends_with('/');
            match (a_is_dir, b_is_dir) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.cmp(b),
            }
        });

        let truncated = entries.len() > MAX_DIR_ENTRIES;
        if truncated {
            entries.truncate(MAX_DIR_ENTRIES);
        }

        let count = if truncated {
            MAX_DIR_ENTRIES
        } else {
            entries.len()
        };

        Ok(ListDirResponse {
            path: path.to_string_lossy().to_string(),
            entries,
            count,
            truncated,
        })
    }
}

#[async_trait]
impl NamedTool for ListDirTool {
    fn name(&self) -> &str {
        "list_dir"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "list_dir".to_string(),
            description: "List contents of a directory on the filesystem.".to_string(),
            parameters: serde_json::to_value(schemars::schema_for!(ListDirArgs))
                .unwrap_or_else(|_| serde_json::json!({"type": "object"})),
        }
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Medium
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: Arc<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError> {
        let response = self.execute_impl(input).await.map_err(ToolError::from)?;
        serialize_tool_output(Self::TOOL_NAME, response)
            .map_err(ListDirToolError::from)
            .map_err(ToolError::from)
    }
}

/// Recursively list directory contents.
async fn list_dir_inner(
    base: &Path,
    path: &Path,
    recursive: bool,
    max_depth: usize,
    current_depth: usize,
    entries: &mut Vec<String>,
) -> Result<(), ListDirToolError> {
    if entries.len() >= MAX_DIR_ENTRIES {
        return Ok(());
    }

    let mut dir = tokio::fs::read_dir(path)
        .await
        .map_err(ListDirToolError::ReadDirectoryFailed)?;

    while let Some(entry) = dir
        .next_entry()
        .await
        .map_err(ListDirToolError::ReadEntryFailed)?
    {
        if entries.len() >= MAX_DIR_ENTRIES {
            break;
        }

        let entry_path = entry.path();
        let relative = entry_path
            .strip_prefix(base)
            .unwrap_or(&entry_path)
            .to_string_lossy()
            .to_string();

        let metadata = entry.metadata().await.ok();
        let is_dir = metadata.as_ref().is_some_and(|m| m.is_dir());

        let display = if is_dir {
            format!("{}/", relative)
        } else {
            let size = metadata.as_ref().map(|m| m.len()).unwrap_or(0);
            format!("{} ({}B)", relative, size)
        };

        entries.push(display);

        if recursive && is_dir && current_depth < max_depth {
            let name_str = entry.file_name().to_string_lossy().to_string();
            if !SKIP_DIRS.contains(&name_str.as_str()) {
                Box::pin(list_dir_inner(
                    base,
                    &entry_path,
                    recursive,
                    max_depth,
                    current_depth + 1,
                    entries,
                ))
                .await?;
            }
        }
    }

    Ok(())
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
    fn test_list_dir_name() {
        let tool = ListDirTool::new();
        assert_eq!(tool.name(), "list_dir");
    }

    #[test]
    fn test_list_dir_risk_level() {
        let tool = ListDirTool::new();
        assert_eq!(tool.risk_level(), RiskLevel::Medium);
    }

    #[tokio::test]
    async fn test_list_dir_basic() {
        let temp_dir = tempfile::tempdir().unwrap();
        std::fs::write(temp_dir.path().join("file1.txt"), "content").unwrap();
        std::fs::write(temp_dir.path().join("file2.txt"), "content").unwrap();
        std::fs::create_dir(temp_dir.path().join("subdir")).unwrap();

        let tool = ListDirTool::new();
        let result = tool
            .execute(
                json!({"path": temp_dir.path().to_str().unwrap()}),
                make_ctx(),
            )
            .await
            .unwrap();

        let entries = result["entries"].as_array().unwrap();
        assert_eq!(entries.len(), 3);
        // Directories should come first
        assert!(entries[0].as_str().unwrap().ends_with('/'));
    }

    #[tokio::test]
    async fn test_list_dir_recursive() {
        let temp_dir = tempfile::tempdir().unwrap();
        std::fs::write(temp_dir.path().join("root.txt"), "root").unwrap();
        std::fs::create_dir(temp_dir.path().join("subdir")).unwrap();
        std::fs::write(temp_dir.path().join("subdir").join("nested.txt"), "nested").unwrap();

        let tool = ListDirTool::new();
        let result = tool
            .execute(
                json!({
                    "path": temp_dir.path().to_str().unwrap(),
                    "recursive": true,
                    "max_depth": 3
                }),
                make_ctx(),
            )
            .await
            .unwrap();

        let entries = result["entries"].as_array().unwrap();
        assert!(
            entries
                .iter()
                .any(|e| e.as_str().unwrap().contains("root.txt"))
        );
        assert!(
            entries
                .iter()
                .any(|e| e.as_str().unwrap().contains("subdir/"))
        );
        assert!(
            entries
                .iter()
                .any(|e| e.as_str().unwrap().contains("nested.txt"))
        );
    }

    #[tokio::test]
    async fn test_list_dir_skips_common_dirs() {
        let temp_dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(temp_dir.path().join("node_modules/foo")).unwrap();
        std::fs::create_dir_all(temp_dir.path().join("target/bar")).unwrap();
        std::fs::create_dir_all(temp_dir.path().join(".git/objects")).unwrap();
        std::fs::write(temp_dir.path().join("main.rs"), "fn main() {}").unwrap();

        let tool = ListDirTool::new();
        let result = tool
            .execute(
                json!({
                    "path": temp_dir.path().to_str().unwrap(),
                    "recursive": true
                }),
                make_ctx(),
            )
            .await
            .unwrap();

        let entries = result["entries"].as_array().unwrap();
        // main.rs should be present
        assert!(
            entries
                .iter()
                .any(|e| e.as_str().unwrap().contains("main.rs"))
        );
        // node_modules, target, .git nested content should NOT appear
        let joined = entries
            .iter()
            .map(|e| e.as_str().unwrap())
            .collect::<String>();
        assert!(!joined.contains("node_modules/foo"));
        assert!(!joined.contains("target/bar"));
        assert!(!joined.contains(".git/objects"));
    }
}
