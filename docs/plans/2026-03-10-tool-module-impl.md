# Tool Module Implementation Plan

**Goal:** Implement the tool module for registering and managing tools used by agents and LLMs.

**Architecture:** Follow existing patterns (LlmProvider trait, LLMManager). Use DashMap for concurrent tool registry.

**Tech Stack:** Rust, async-trait, dashmap, serde_json

---

## Task 1: Add dashmap dependency

**Files:**
- Modify: `crates/claw/Cargo.toml`

**Changes:**
Add to `[dependencies]`:
```toml
dashmap = "6"
```

**Commands:**
```bash
cargo check -p claw
```

**Expected output:** Dependency resolved successfully.

---

## Task 2: Create tool module with NamedTool trait and ToolError

**Files:**
- Create: `crates/claw/src/tool/mod.rs`

**Code:**
```rust
//! Tool module: defines NamedTool trait and ToolManager for agent/LLM tool management.

use async_trait::async_trait;
use dashmap::DashMap;
use std::sync::Arc;

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
```

**Commands:**
```bash
cargo check -p claw
```

**Expected output:** Compiles successfully.

---

## Task 3: Implement ToolManager
**Files:**
- Modify: `crates/claw/src/tool/mod.rs`

**Code to append:**
```rust
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
```

**Commands:**
```bash
cargo check -p claw
```

**Expected output:** Compiles successfully.

---

## Task 4: Update lib.rs exports
**Files:**
- Modify: `crates/claw/src/lib.rs`

**Changes:**
Add `pub mod tool;` and re-exports. Final file:
```rust
pub mod agents;
pub mod claw;
pub mod db;
pub mod error;
pub mod llm;
pub mod tool;

pub use claw::AppContext;
pub use error::AgentError;
pub use tool::{NamedTool, ToolError, ToolManager};
```

**Commands:**
```bash
cargo check -p claw
```

**Expected output:** Compiles successfully.

---

## Task 5: Add ToolError variant to AgentError
**Files:**
- Modify: `crates/claw/src/error.rs`

**Changes:**
Add new variant. First read the existing file to see current structure, then add:
```rust
#[error("Tool error: {0}")]
Tool(#[from] crate::tool::ToolError),
```

**Commands:**
```bash
cargo check -p claw
```

**Expected output:** Compiles successfully.

---

## Task 6: Update AppContext with ToolManager
**Files:**
- Modify: `crates/claw/src/claw.rs`

**Changes:**
1. Add import: `use crate::tool::ToolManager;`
2. Add field to AppContext: `tool_manager: Arc<ToolManager>,`
3. Initialize in `init()`: `let tool_manager = Arc::new(ToolManager::new());`
4. Add accessor: `pub fn tool_manager(&self) -> Arc<ToolManager>`
5. Update `new()` constructor

**Commands:**
```bash
cargo check -p claw
```

**Expected output:** Compiles successfully.

---

## Task 7: Add unit tests for ToolManager
**Files:**
- Modify: `crates/claw/src/tool/mod.rs`

**Code to append at end of file:**
```rust
#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn register_and_get_tool() {
        let manager = ToolManager::new();
        let tool = Arc::new(EchoTool);

        manager.register(tool);
        let retrieved = manager.get("echo");

        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().name(), "echo");
    }

    #[test]
    fn get_nonexistent_tool_returns_none() {
        let manager = ToolManager::new();
        let result = manager.get("nonexistent");
        assert!(result.is_none());
    }

    #[test]
    fn list_definitions_returns_all() {
        let manager = ToolManager::new();
        manager.register(Arc::new(EchoTool));

        let defs = manager.list_definitions();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name, "echo");
    }

    #[tokio::test]
    async fn execute_tool_returns_result() {
        let manager = ToolManager::new();
        manager.register(Arc::new(EchoTool));

        let args = serde_json::json!({"message": "hello"});
        let result = manager.execute("echo", args.clone()).await;

        assert!(result.is_ok());
        assert_eq!(result.unwrap(), args);
    }

    #[tokio::test]
    async fn execute_nonexistent_tool_returns_error() {
        let manager = ToolManager::new();

        let result = manager.execute("nonexistent", serde_json::json!({})).await;
        assert!(matches!(result, Err(ToolError::NotFound { .. })));
    }
}
```

**Commands:**
```bash
cargo test -p claw --lib
```

**Expected output:** All tests pass.

---

## Task 8: Run clippy and format
**Files:**
- N/A (validation only)

**Commands:**
```bash
cargo fmt
cargo clippy --all --benches --tests --examples --all-features
```

**Expected output:** Zero warnings.

---

## Task 9: Final verification
**Commands:**
```bash
cargo test -p claw
cargo check -p claw --all-features
```

**Expected output:** All tests pass, no compilation errors.

---

## Commit Strategy

Commit after each task or logical group:
1. Tasks 1-3: "feat(claw): add tool module with NamedTool trait and ToolManager"
2. Tasks 4-6: "feat(claw): integrate tool module with AppContext"
3. Task 7-9: "test(claw): add unit tests for ToolManager"
