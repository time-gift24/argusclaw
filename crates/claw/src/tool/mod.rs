//! Tool module: defines NamedTool trait and ToolError for agent/LLM tool management.

use std::sync::Arc;

use async_trait::async_trait;
use dashmap::DashMap;

use crate::llm::ToolDefinition;

/// Error type for tool operations.
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// Tool not found in registry.
    #[error("Tool not found: {id}")]
    NotFound { id: String },

    /// Tool execution failed.
    #[error("Tool '{tool_name}' execution failed: {reason}")]
    ExecutionFailed { tool_name: String, reason: String },
}

/// Trait for defining tools that can be used by agents and LLMs.
#[async_trait]
pub trait NamedTool: Send + Sync {
    /// Returns the unique name of the tool.
    fn name(&self) -> &str;

    /// Returns the tool definition for LLM consumption.
    fn definition(&self) -> ToolDefinition;

    /// Execute the tool with the provided arguments.
    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value, ToolError>;
}

/// Registry for tools with concurrent access support.
pub struct ToolManager {
    tools: DashMap<String, Arc<dyn NamedTool>>,
}

impl ToolManager {
    #[must_use]
    pub fn new() -> Self {
        Self {
            tools: DashMap::new(),
        }
    }

    /// Register a tool. Overwrites if tool with same name already exists.
    pub fn register(&self, tool: Arc<dyn NamedTool>) {
        self.tools.insert(tool.name().to_string(), tool);
    }

    /// Get a tool by name.
    pub fn get(&self, name: &str) -> Option<Arc<dyn NamedTool>> {
        self.tools.get(name).map(|v| v.clone())
    }

    /// List all registered tool definitions for LLM consumption.
    pub fn list_definitions(&self) -> Vec<ToolDefinition> {
        self.tools.iter().map(|e| e.definition()).collect()
    }

    /// Execute a tool by name with the provided arguments.
    pub async fn execute(
        &self,
        name: &str,
        args: serde_json::Value,
    ) -> Result<serde_json::Value, ToolError> {
        let tool = self.get(name).ok_or_else(|| ToolError::NotFound {
            id: name.to_string(),
        })?;
        tool.execute(args).await
    }
}

impl Default for ToolManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A test tool that echoes back its input arguments.
    struct EchoTool;

    #[async_trait]
    impl NamedTool for EchoTool {
        fn name(&self) -> &str {
            "echo"
        }

        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "echo".to_string(),
                description: "Echoes back the input".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "message": { "type": "string" }
                    }
                }),
            }
        }

        async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value, ToolError> {
            Ok(args)
        }
    }

    /// A test tool that always fails with ExecutionFailed error.
    struct FailingTool;

    #[async_trait]
    impl NamedTool for FailingTool {
        fn name(&self) -> &str {
            "failing"
        }

        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "failing".to_string(),
                description: "Always fails".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            }
        }

        async fn execute(&self, _args: serde_json::Value) -> Result<serde_json::Value, ToolError> {
            Err(ToolError::ExecutionFailed {
                tool_name: "failing".to_string(),
                reason: "intentional failure".to_string(),
            })
        }
    }

    /// Another test tool for testing multiple tool registration.
    struct AnotherTool;

    #[async_trait]
    impl NamedTool for AnotherTool {
        fn name(&self) -> &str {
            "another"
        }

        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "another".to_string(),
                description: "Another test tool".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            }
        }

        async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value, ToolError> {
            Ok(serde_json::json!({"processed": args}))
        }
    }

    /// A tool with a configurable name for testing overwrite behavior.
    struct NamedTestTool {
        name: String,
        value: i32,
    }

    #[async_trait]
    impl NamedTool for NamedTestTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: self.name.clone(),
                description: format!("Test tool with value {}", self.value),
                parameters: serde_json::json!({"type": "object"}),
            }
        }

        async fn execute(&self, _args: serde_json::Value) -> Result<serde_json::Value, ToolError> {
            Ok(serde_json::json!({"value": self.value}))
        }
    }

    #[test]
    fn register_get_and_overwrite() {
        let manager = ToolManager::new();

        // Register initial tool
        let tool_v1 = Arc::new(NamedTestTool {
            name: "test".to_string(),
            value: 1,
        });
        manager.register(tool_v1);

        // Verify initial registration
        let retrieved = manager.get("test");
        assert!(retrieved.is_some());
        assert_eq!(retrieved.as_ref().unwrap().name(), "test");

        // Overwrite with new version
        let tool_v2 = Arc::new(NamedTestTool {
            name: "test".to_string(),
            value: 2,
        });
        manager.register(tool_v2);

        // Verify overwrite - should have new value
        let retrieved = manager.get("test");
        assert!(retrieved.is_some());

        // Execute to verify we got the new version
        let rt = tokio::runtime::Runtime::new().unwrap();
        let result = rt.block_on(retrieved.unwrap().execute(serde_json::json!({})));
        assert!(result.is_ok());
        assert_eq!(result.unwrap()["value"], 2);
    }

    #[test]
    fn list_definitions_returns_definitions() {
        let manager = ToolManager::new();

        // Initially empty
        let defs = manager.list_definitions();
        assert!(defs.is_empty());

        // Register tools
        manager.register(Arc::new(EchoTool));
        manager.register(Arc::new(AnotherTool));

        let defs = manager.list_definitions();
        assert_eq!(defs.len(), 2);

        // Verify definitions are present
        let names: Vec<&str> = defs.iter().map(|d| d.name.as_str()).collect();
        assert!(names.contains(&"echo"));
        assert!(names.contains(&"another"));

        // Verify definition content
        let echo_def = defs.iter().find(|d| d.name == "echo").unwrap();
        assert_eq!(echo_def.description, "Echoes back the input");
    }

    #[tokio::test]
    async fn execute_success() {
        let manager = ToolManager::new();
        manager.register(Arc::new(EchoTool));

        let args = serde_json::json!({"message": "hello"});
        let result = manager.execute("echo", args.clone()).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), args);
    }

    #[tokio::test]
    async fn execute_error_not_found() {
        let manager = ToolManager::new();

        let result = manager.execute("nonexistent", serde_json::json!({})).await;

        assert!(result.is_err());
        match result {
            Err(ToolError::NotFound { id }) => assert_eq!(id, "nonexistent"),
            _ => panic!("Expected NotFound error"),
        }
    }

    #[tokio::test]
    async fn execute_error_execution_failed() {
        let manager = ToolManager::new();
        manager.register(Arc::new(FailingTool));

        let result = manager.execute("failing", serde_json::json!({})).await;

        assert!(result.is_err());
        match result {
            Err(ToolError::ExecutionFailed { tool_name, reason }) => {
                assert_eq!(tool_name, "failing");
                assert_eq!(reason, "intentional failure");
            }
            _ => panic!("Expected ExecutionFailed error"),
        }
    }

    #[test]
    fn default_trait() {
        let manager = ToolManager::default();

        // Default should create an empty manager
        let defs = manager.list_definitions();
        assert!(defs.is_empty());

        // Should be able to register tools
        manager.register(Arc::new(EchoTool));
        assert!(manager.get("echo").is_some());
    }
}
