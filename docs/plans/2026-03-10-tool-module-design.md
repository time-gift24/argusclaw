# Tool Module Design

**Date:** 2026-03-10
**Status:** Approved

## Overview

Add a `tool` module to `crates/claw` that defines a `NamedTool` trait and `ToolManager` for registering and managing tools used by agents and LLMs.

## Requirements

1. `NamedTool` trait with:
   - `name()` — globally unique identifier within ToolManager scope
   - `definition()` — returns `ToolDefinition` for LLM consumption
   - `execute()` — executes the tool with JSON arguments

2. `ToolManager` for:
   - Registering tools
   - Querying tools by name
   - Listing tool definitions for LLM
   - Executing tools by name

3. Tool ID uniqueness scope: within a ToolManager instance

4. Tool execution model: Tool trait defines both definition and execution logic

## Module Structure

```
crates/claw/src/
├── tool/
│   └── mod.rs          # NamedTool trait, ToolManager, ToolError
├── lib.rs              # Add pub mod tool;
├── claw.rs             # Add ToolManager to AppContext
└── error.rs            # Add ToolError variant to AgentError
```

## Core Types

### NamedTool Trait

```rust
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

pub use crate::llm::ToolDefinition;

#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("Tool not found: {id}")]
    NotFound { id: String },

    #[error("Tool '{tool_name}' execution failed: {reason}")]
    ExecutionFailed { tool_name: String, reason: String },
}

#[async_trait]
pub trait NamedTool: Send + Sync {
    fn name(&self) -> &str;
    fn definition(&self) -> ToolDefinition;
    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value, ToolError>;
}
```

### ToolManager

```rust
use dashmap::DashMap;

pub struct ToolManager {
    tools: DashMap<String, Arc<dyn NamedTool>>,
}

impl ToolManager {
    pub fn new() -> Self;
    pub fn register(&self, tool: Arc<dyn NamedTool>);
    pub fn get(&self, name: &str) -> Option<Arc<dyn NamedTool>>;
    pub fn list_definitions(&self) -> Vec<ToolDefinition>;
    pub async fn execute(&self, name: &str, args: serde_json::Value) -> Result<serde_json::Value, ToolError>;
}
```

## Integration

### lib.rs

```rust
pub mod tool;

pub use tool::{NamedTool, ToolManager, ToolError};
```

### claw.rs (AppContext)

```rust
use crate::tool::ToolManager;

pub struct AppContext {
    llm_manager: Arc<LLMManager>,
    agent_manager: Arc<AgentManager>,
    tool_manager: Arc<ToolManager>,
}

impl AppContext {
    pub async fn init(database_target: Option<String>) -> Result<Self, AgentError> {
        // ... existing init code ...
        let tool_manager = Arc::new(ToolManager::new());
        Ok(Self::new(llm_manager, agent_manager, tool_manager))
    }

    pub fn new(
        llm_manager: Arc<LLMManager>,
        agent_manager: Arc<AgentManager>,
        tool_manager: Arc<ToolManager>,
    ) -> Self {
        Self { llm_manager, agent_manager, tool_manager }
    }

    pub fn tool_manager(&self) -> Arc<ToolManager> {
        Arc::clone(&self.tool_manager)
    }
}
```

### error.rs

Add to `AgentError`:

```rust
#[error("Tool error: {0}")]
Tool(#[from] crate::tool::ToolError),
```

### Cargo.toml

Add dependency:

```toml
dashmap = "6"
```

## Design Decisions

| Decision | Rationale |
|----------|-----------|
| `NamedTool` naming | Avoids conflict with `std::tool` |
| `name()` instead of `id()` | Clearer semantic meaning |
| `DashMap` instead of `RwLock<HashMap>` | Simpler `&self` registration, better concurrency |
| Reuse `ToolDefinition` from `llm/provider.rs` | No duplication, single source of truth |
| Tool ID scope: per-ToolManager | Simpler, supports isolated test scenarios |
