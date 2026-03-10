# Turn Module Design Document

**Date**: 2026-03-10
**Author**: ArgusClaw Team

## Overview

This document describes the design of the `turn` module, which represents a single turn in a conversation where an LLM can autonom call tools to complete tasks. A turn consists of:

1. Receiving historical messages and system prompt
2. Calling the LLM with available tools
3. Executing tool calls in parallel
4. Returning tool results to the LLM
5. Repeating until the LLM returns a final response

## Scope

**In Scope:**
- Single turn execution (LLM → Tool → LLM cycles)
- Tool execution with timeout and parallel support
- Hook system for extensibility

**Out of Scope:**
- Multi-turn conversation history management (handled by Session layer)
- Context compaction (handled by Session layer)
- Message persistence

## Design Decisions

### 1. Modular Builder Pattern

Use `derive_builder` to generate configuration and input types with compile-time defaults and validation.

**Rationale:**
- Type-safe configuration with defaults
- Builder pattern for complex construction
- Avoids runtime errors from missing fields

### 2. Minimal Configuration

Three core parameters with sensible defaults:

```rust
TurnConfig {
    max_tool_calls: 10,        // Maximum tool calls per turn
    tool_timeout_secs: 120,    // Tool execution timeout (seconds)
    max_iterations: 50,       // Maximum loop iterations
}
```

### 3. Clear Responsibility Boundary

The `turn` module focuses solely on turn execution. Higher-level concerns (history management, compaction) are handled by the `Session` or `Conversation` layer.

**Benefits:**
- Single responsibility per module
- Easier testing and maintenance
- Clear separation of concerns

### 4. Parallel Tool Execution

Multiple tool calls are executed concurrently, with results collected and returned together.

**Implementation:**
- Use `tokio::time::timeout` for each tool execution
- Execute all tools concurrently using `futures::future::join_all`
- Collect results in order

### 5. Hooks System

Extensible hook system for intercepting turn events.

**Hook Events:**
- `BeforeToolCall`: Fires before tool execution. Can block the call by returning `Err`.
- `AfterToolCall`: Fires after tool execution. Observe-only.
- `TurnEnd`: Fires after turn completes. Observe-only.

### 6. Error Handling

Tool execution failures are returned as error information in the tool result message, allowing the LLM to handle and recover.

**Implementation:**
- On tool failure, create tool result message with error information
- Continue execution to next iteration

## Module Structure

```
crates/claw/src/agents/
├── mod.rs           # Module entry
└── turn/
    ├── mod.rs          # Turn module entry
    ├── config.rs      # TurnConfig, TurnInput, TurnOutput
    ├── error.rs       # TurnError
    ├── hooks.rs       # Hook trait + HookRegistry
    ├── builder.rs     # Re-export builder types
    └── execution.rs   # Core execution logic
```

## Core Types

### TurnConfig

Configuration for turn execution with sensible defaults.

```rust
#[derive(Debug, Clone, Builder)]
#[builder(setter(skip))]
pub struct TurnConfig {
    /// Maximum tool calls per turn.
    #[builder(default = Some(10))]
    max_tool_calls: u32,

    /// Maximum duration for a single tool execution (seconds).
    #[builder(default = Some(120))]
    tool_timeout_secs: u64,

    /// Maximum number of loop iterations (LLM → Tool → LLM cycles).
    #[builder(default = Some(50))]
    max_iterations: u32,
}

impl TurnConfig {
    pub fn new() -> Self {
        Self {
            max_tool_calls: Some(10),
            tool_timeout_secs: Some(120),
            max_iterations: Some(50),
        }
    }
}
```

### TurnInput

Input for turn execution.

```rust
#[derive(Debug, Clone, Builder)]
#[builder(setter(into = |skip))]
pub struct TurnInput {
    /// Historical messages for the conversation.
    #[builder(setter(into = Vec::new()))]
    pub messages: Vec<ChatMessage>,

    /// System prompt for this turn.
    #[builder(setter(into = String::new()))]
    pub system_prompt: String,

    /// LLM provider instance.
    pub provider: Arc<dyn LlmProvider>,

    /// Tool manager for registry.
    #[builder(setter(into = |skip))]
    pub tool_manager: Arc<ToolManager>,

    /// Tool IDs to use (resolved via ToolManager).
    #[builder(setter(into = Vec::new()))]
    pub tool_ids: Vec<String>,

    /// Optional hook registry.
    #[builder(default = None, setter(into = Option::None))]
    pub hooks: Option<Arc<HookRegistry>>,
}
```

### TurnOutput

Output from turn execution.

```rust
#[derive(Debug, Clone, Builder)]
#[builder(setter(skip))]
pub struct TurnOutput {
    /// Updated message history (includes assistant response + tool results).
    pub messages: Vec<ChatMessage>,

    /// Token usage statistics.
    #[builder(default)]
    pub token_usage: TokenUsage,
}

#[derive(Debug, Clone, Default)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
}
```

### TurnError

Errors during turn execution.

```rust
#[derive(Debug, Error)]
pub enum TurnError {
    #[error("LLM call failed: {0}")]
    LlmFailed(#[from] crate::llm::LlmError),

    #[error("Tool not found: {0}")]
    ToolNotFound(String),

    #[error("Tool '{name}' execution failed: {reason}")]
    ToolExecutionFailed { name: String, reason: String },

    #[error("Tool call blocked by hook: {reason}")]
    ToolCallBlocked { reason: String },

    #[error("Maximum iterations ({0}) reached")]
    MaxIterationsReached(u32),

    #[error("Context length exceeded: {0} tokens")]
    ContextLengthExceeded(usize),

    #[error("Turn timeout exceeded")]
    TimeoutExceeded,
}
```

### HookEvent & HookHandler

Hook system for extensibility.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookEvent {
    BeforeToolCall,
    AfterToolCall,
    TurnEnd,
}

pub struct HookContext {
    pub tool_name: String,
    pub tool_call_id: String,
    pub tool_input: serde_json::Value,
    pub tool_result: Option<serde_json::Value>,
    pub error: Option<String>,
}

#[async_trait]
pub trait HookHandler: Send + Sync {
    async fn on_event(&self, ctx: &HookContext) -> Result<(), String>;
}

#[derive(Default)]
pub struct HookRegistry {
    handlers: DashMap<HookEvent, Vec<Arc<dyn HookHandler>>>,
}
```

## Execution Flow

```
┌─────────────────┐
│ TurnInput      │
│ (messages,     │
│  system_prompt,│
│  provider,      │
│  tool_ids)      │
└────────┬────────┘
       │
       ▼
       │
┌─────────────────┐
│ Build Request   │
└────────┬────────┘
       │
       ▼
       │
┌─────────────────┐    ┌──────────────────┐
│ Call LLM       │──▶│ ToolCompletionRequest │
└────────┬────────┘    └──────────────────┘
       │
       ▼
       │
┌─────────────────────┐
│ Check finish_reason│
└────────┬───────────┘
       │
   ┌───Stop───┬───ToolUse───┬───Length───┐
       │              │              │
       ▼              ▼              │
       │              │              │
       │         ┌──────────────┐
       │         │ Execute Tools │
       │         │ (parallel)    │
       │         └──────────────┘
       │              │
       ▼              │
       │         ┌──────────────┐
       │         │ Fire Hooks    │
       │         └──────────────┘
       │              │
       ▼              │
       │         ┌──────────────┐
       │         │ Add Results    │
       │         │ to Messages   │
       │         └──────────────┘
       │              │
       ▼              │
       └──────────────────┘
                   │
                   ▼
            Loop back to
            Call LLM
```

## API Design

### execute_turn

Main entry point for turn execution.

```rust
pub async fn execute_turn(
    input: TurnInput,
    config: TurnConfig,
) -> Result<TurnOutput, TurnError>;
```

### Design Principles
1. **Single Responsibility**: Turn handles only turn execution
2. **Configuration over Hardcoding**: Use TurnConfig for flexibility
3. **Extensibility**: Hook system for customization
4. **Error Propagation**: Return errors, not panic
5. **Parallel Execution**: Execute tools concurrently for performance
