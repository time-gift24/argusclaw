//! Shell tool implementation for executing shell commands.
//!
//! This tool executes shell commands asynchronously with the following parameters:
//! - `command` (required): The command to execute
//! - `timeout` (optional): Timeout in seconds (default: 120)
//! - `cwd` (optional): Working directory (default: current directory)
//!
//! # Security
//!
//! Shell commands are dangerous operations. This tool has `RiskLevel::Critical`
//! and requires approval by default.
//!
//! # Example
//!
//! ```json
//! {
//!   "command": "echo 'Hello, World!'"
//! }
//! ```
//!
//! Output:
//! ```json
//! {
//!   "stdout": "Hello, World!\n",
//!   "stderr": "",
//!   "exit_code": 0
//! }
//! ```

use async_trait::async_trait;
use serde_json::json;
use std::sync::Arc;
use tokio::process::Command;

use argus_protocol::llm::ToolDefinition;
use argus_protocol::risk_level::RiskLevel;
use argus_protocol::{NamedTool, ToolError, ToolExecutionContext};

/// Default timeout for shell commands in seconds.
const DEFAULT_TIMEOUT_SECS: u64 = 120;

/// Shell tool implementation - executes shell commands with risk level Critical.
pub struct ShellTool {
    timeout_secs: u64,
}

impl Default for ShellTool {
    fn default() -> Self {
        Self::new()
    }
}

impl ShellTool {
    /// Create a new ShellTool with default timeout.
    #[must_use]
    pub fn new() -> Self {
        Self {
            timeout_secs: DEFAULT_TIMEOUT_SECS,
        }
    }

    /// Create with custom timeout.
    #[must_use]
    pub fn with_timeout(timeout_secs: u64) -> Self {
        Self { timeout_secs }
    }
}

#[async_trait]
impl NamedTool for ShellTool {
    fn name(&self) -> &str {
        "shell"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "shell".to_string(),
            description: "Execute a shell command".to_string(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "command": {
                        "type": "string",
                        "description": "The command to execute"
                    },
                    "timeout": {
                        "type": "number",
                        "description": "Optional timeout in seconds (default: 120)"
                    },
                    "cwd": {
                        "type": "string",
                        "description": "Working directory (default: current directory)"
                    }
                },
                "required": ["command"]
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Critical
    }

    async fn execute(&self, input: serde_json::Value, _ctx: Arc<ToolExecutionContext>) -> Result<serde_json::Value, ToolError> {
        // Parse command argument (required)
        let command = input
            .get("command")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool_name: "shell".to_string(),
                reason: "Missing required parameter: command".to_string(),
            })?;

        // Parse timeout (optional)
        let timeout_secs = input
            .get("timeout")
            .and_then(|v| v.as_u64())
            .unwrap_or(self.timeout_secs);

        // Parse working directory (optional)
        let cwd = input
            .get("cwd")
            .and_then(|v| v.as_str())
            .map(std::path::PathBuf::from);

        // Build the command
        let mut cmd = Command::new("sh");
        cmd.arg("-c").arg(command);

        // Set working directory if provided
        if let Some(dir) = &cwd {
            cmd.current_dir(dir);
        }

        // Execute with timeout
        let timeout_duration = std::time::Duration::from_secs(timeout_secs);
        let result = tokio::time::timeout(timeout_duration, cmd.output()).await;

        match result {
            Ok(Ok(output)) => {
                // Success - return structured output
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let exit_code = output.status.code();

                Ok(json!({
                    "stdout": stdout,
                    "stderr": stderr,
                    "exit_code": exit_code
                }))
            }
            Ok(Err(e)) => {
                // Command execution failed
                Err(ToolError::ExecutionFailed {
                    tool_name: "shell".to_string(),
                    reason: format!("Failed to execute command: {}", e),
                })
            }
            Err(_elapsed) => {
                // Timeout
                Err(ToolError::ExecutionFailed {
                    tool_name: "shell".to_string(),
                    reason: format!("Command timed out after {}s", timeout_secs),
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use argus_protocol::ids::ThreadId;
    use tokio::sync::broadcast;

    fn make_ctx() -> Arc<ToolExecutionContext> {
        let (tx, _) = broadcast::channel(16);
        Arc::new(ToolExecutionContext {
            thread_id: ThreadId::new_v4(),
            pipe_tx: tx,
        })
    }

    #[test]
    fn test_shell_tool_name() {
        let tool = ShellTool::new();
        assert_eq!(tool.name(), "shell");
    }

    #[test]
    fn test_shell_tool_risk_level() {
        let tool = ShellTool::new();
        assert_eq!(tool.risk_level(), RiskLevel::Critical);
    }

    #[test]
    fn test_shell_tool_definition() {
        let tool = ShellTool::new();
        let def = tool.definition();
        assert_eq!(def.name, "shell");
        assert!(
            def.parameters["required"]
                .as_array()
                .unwrap()
                .contains(&json!("command"))
        );
    }

    #[tokio::test]
    async fn test_shell_tool_echo() {
        let tool = ShellTool::new();
        let result = tool
            .execute(json!({"command": "echo 'test'"}), make_ctx())
            .await
            .unwrap();

        assert_eq!(result["exit_code"], 0);
        assert!(result["stdout"].as_str().unwrap().contains("test"));
    }

    #[tokio::test]
    async fn test_shell_tool_missing_command() {
        let tool = ShellTool::new();
        let result = tool.execute(json!({}), make_ctx()).await;

        assert!(result.is_err());
        match result {
            Err(ToolError::ExecutionFailed { tool_name, reason }) => {
                assert_eq!(tool_name, "shell");
                assert!(reason.contains("Missing required parameter"));
            }
            _ => panic!("Expected ExecutionFailed error"),
        }
    }
}
