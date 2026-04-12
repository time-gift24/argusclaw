//! Argus Tool crate - Tool system for agents and LLMs.
//!
//! This crate provides the tool system including the NamedTool trait, ToolManager,
//! and built-in tool implementations.

use std::sync::Arc;

use dashmap::DashMap;

use argus_protocol::llm::ToolDefinition;
use argus_protocol::risk_level::RiskLevel;

// Re-export from argus_protocol
pub use argus_protocol::{NamedTool, ToolError, ToolExecutionContext};

mod chrome;
pub mod clap_tool;
pub mod glob;
pub mod grep;
pub mod http;
pub mod list;
pub mod patch;
pub mod path_utils;
pub mod read;
pub mod scheduler;
pub mod shell;
pub mod write;

pub use chrome::ChromeTool;
pub use clap_tool::{ClapExecutor, ClapTool};
pub use glob::GlobTool;
pub use grep::GrepTool;
pub use http::HttpTool;
pub use list::ListDirTool;
pub use patch::ApplyPatchTool;
pub use read::ReadTool;
pub use scheduler::{
    CheckInboxRequest, MAX_DISPATCH_DEPTH, MarkReadRequest, SchedulerBackend,
    SchedulerDispatchRequest, SchedulerJobLookup, SchedulerJobResult, SchedulerLookupRequest,
    SchedulerSubagent, SchedulerTool, SendMessageRequest, SendMessageResponse,
};
pub use shell::ShellTool;
pub use write::WriteFileTool;

/// Registry for tools with concurrent access support.
pub struct ToolManager {
    tools: DashMap<String, Arc<dyn NamedTool>>,
}

impl std::fmt::Debug for ToolManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let keys: Vec<String> = self.tools.iter().map(|e| e.key().clone()).collect();
        f.debug_struct("ToolManager").field("tools", &keys).finish()
    }
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
        ctx: Arc<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError> {
        let tool = self.get(name).ok_or_else(|| ToolError::NotFound {
            id: name.to_string(),
        })?;
        tool.execute(args, ctx).await
    }

    /// Get the risk level for a tool by name.
    /// Returns `RiskLevel::Low` if the tool is not found.
    pub fn get_risk_level(&self, name: &str) -> RiskLevel {
        self.get(name)
            .map(|t| t.risk_level())
            .unwrap_or(RiskLevel::Low)
    }

    /// List all registered tool IDs (names).
    pub fn list_ids(&self) -> Vec<String> {
        self.tools.iter().map(|e| e.key().clone()).collect()
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

    // Import async_trait to implement NamedTool from argus_protocol
    use async_trait::async_trait;

    #[test]
    fn re_exports_max_dispatch_depth() {
        assert_eq!(MAX_DISPATCH_DEPTH, scheduler::MAX_DISPATCH_DEPTH);
    }

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

        async fn execute(
            &self,
            input: serde_json::Value,
            _ctx: Arc<ToolExecutionContext>,
        ) -> Result<serde_json::Value, ToolError> {
            Ok(input)
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

        async fn execute(
            &self,
            _input: serde_json::Value,
            _ctx: Arc<ToolExecutionContext>,
        ) -> Result<serde_json::Value, ToolError> {
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

        async fn execute(
            &self,
            input: serde_json::Value,
            _ctx: Arc<ToolExecutionContext>,
        ) -> Result<serde_json::Value, ToolError> {
            Ok(serde_json::json!({"processed": input}))
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

        async fn execute(
            &self,
            _input: serde_json::Value,
            _ctx: Arc<ToolExecutionContext>,
        ) -> Result<serde_json::Value, ToolError> {
            Ok(serde_json::json!({"value": self.value}))
        }
    }

    /// A tool with configurable risk level.
    struct RiskyTool {
        name: String,
        risk: RiskLevel,
    }

    #[async_trait]
    impl NamedTool for RiskyTool {
        fn name(&self) -> &str {
            &self.name
        }

        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: self.name.clone(),
                description: "A risky tool".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            }
        }

        async fn execute(
            &self,
            _input: serde_json::Value,
            _ctx: Arc<ToolExecutionContext>,
        ) -> Result<serde_json::Value, ToolError> {
            Ok(serde_json::json!({}))
        }

        fn risk_level(&self) -> RiskLevel {
            self.risk
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
        let (tx, _rx) = tokio::sync::broadcast::channel(1);
        let ctx = Arc::new(ToolExecutionContext {
            thread_id: argus_protocol::ids::ThreadId::new(),
            agent_id: None,
            pipe_tx: tx,
        });
        let result = rt.block_on(retrieved.unwrap().execute(serde_json::json!({}), ctx));
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
        let (tx, _rx) = tokio::sync::broadcast::channel(1);
        let ctx = Arc::new(ToolExecutionContext {
            thread_id: argus_protocol::ids::ThreadId::new(),
            agent_id: None,
            pipe_tx: tx,
        });
        let result = manager.execute("echo", args.clone(), ctx).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), args);
    }

    #[tokio::test]
    async fn execute_error_not_found() {
        let manager = ToolManager::new();
        let (tx, _rx) = tokio::sync::broadcast::channel(1);
        let ctx = Arc::new(ToolExecutionContext {
            thread_id: argus_protocol::ids::ThreadId::new(),
            agent_id: None,
            pipe_tx: tx,
        });

        let result = manager
            .execute("nonexistent", serde_json::json!({}), ctx)
            .await;

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
        let (tx, _rx) = tokio::sync::broadcast::channel(1);
        let ctx = Arc::new(ToolExecutionContext {
            thread_id: argus_protocol::ids::ThreadId::new(),
            agent_id: None,
            pipe_tx: tx,
        });

        let result = manager.execute("failing", serde_json::json!({}), ctx).await;

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

    #[test]
    fn default_risk_level_is_low() {
        let manager = ToolManager::new();
        manager.register(Arc::new(EchoTool));

        assert_eq!(manager.get_risk_level("echo"), RiskLevel::Low);
    }

    #[test]
    fn tool_can_override_risk_level() {
        let manager = ToolManager::new();

        manager.register(Arc::new(RiskyTool {
            name: "shell_exec".to_string(),
            risk: RiskLevel::Critical,
        }));
        manager.register(Arc::new(RiskyTool {
            name: "file_write".to_string(),
            risk: RiskLevel::High,
        }));
        manager.register(Arc::new(RiskyTool {
            name: "web_fetch".to_string(),
            risk: RiskLevel::Medium,
        }));

        assert_eq!(manager.get_risk_level("shell_exec"), RiskLevel::Critical);
        assert_eq!(manager.get_risk_level("file_write"), RiskLevel::High);
        assert_eq!(manager.get_risk_level("web_fetch"), RiskLevel::Medium);
    }

    #[test]
    fn get_risk_level_returns_low_for_nonexistent_tool() {
        let manager = ToolManager::new();

        assert_eq!(manager.get_risk_level("nonexistent"), RiskLevel::Low);
    }
}
