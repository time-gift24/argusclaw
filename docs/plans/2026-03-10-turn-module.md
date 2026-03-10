# Turn Module Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Implement a `turn` module that executes a single turn in a conversation where an LLM can autonomously call tools to complete tasks.

**Architecture:** Modular builder pattern for TurnConfig/TurnInput/TurnOutput. Single responsibility - turn execution only. Higher-level concerns (history, compaction) handled by Session layer.
**Tech Stack:** Rust, derive_builder, async_trait, dashmap, thiserror, tokio

---

## Task 1: Module Structure Setup

**Files:**
- Create: `crates/claw/src/agents/mod.rs` (modify)
- Create: `crates/claw/src/agents/turn/mod.rs`
- Create: `crates/claw/src/agents/turn/config.rs`
- Create: `crates/claw/src/agents/turn/error.rs`

**Step 1: Update agents/mod.rs to expose turn module**

```rust
pub mod turn;
```

**Step 2: Run cargo check to verify module compiles**

Run: `cargo check -p claw`
Expected: PASS

**Step 3: Create turn/mod.rs with module declarations**

```rust
mod config;
mod error;
mod hooks;
mod execution;

pub use config::{TurnConfig, TurnConfigBuilder, TurnInput, TurnInputBuilder, TurnOutput, TurnOutputBuilder, TokenUsage};
pub use error::TurnError;
pub use hooks::{HookContext, HookEvent, HookHandler, HookRegistry};
```

**Step 4: Run cargo check**

Run: `cargo check -p claw`
Expected: PASS (module not found errors for submodules)

---

## Task 2: Config Types with Builder Pattern
**Files:**
- Create: `crates/claw/src/agents/turn/config.rs`

**Step 1: Add derive_builder to Cargo.toml if not present**

Run: `grep -q "derive_builder" crates/claw/Cargo.toml`
Expected: If not found, add it.

**Step 2: Write failing test for config.rs**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_turn_config_defaults() {
        let config = TurnConfig::new();
        assert_eq!(config.max_tool_calls, Some(10));
        assert_eq!(config.tool_timeout_secs, Some(120));
        assert_eq!(config.max_iterations, Some(50));
    }
}
```

**Step 3: Run test to verify it fails**

Run: `cargo test -p claw --lib`
Expected: FAIL - TurnConfig not found

**Step 4: Write minimal implementation**

```rust
use derive_builder::Builder;
use std::sync::Arc;

use crate::llm::{ChatMessage, LlmProvider, ToolDefinition, ToolCall};
use crate::tool::ToolManager;

/// Turn execution configuration.
#[derive(Debug, Clone, Builder)]
#[builder(setter(skip))]
pub struct TurnConfig {
    /// Maximum tool calls per turn.
    #[builder(default = Some(10))]
    pub max_tool_calls: Option<u32>,
    /// Maximum duration for a single tool execution (seconds).
    #[builder(default = Some(120))]
    pub tool_timeout_secs: Option<u64>,
    /// Maximum number of loop iterations (LLM -> Tool → LLM cycles).
    #[builder(default = Some(50))]
    pub max_iterations: u32,
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

**Step 5: Run test to verify it passes**

Run: `cargo test -p claw test_turn_config_defaults`
Expected: PASS

**Step 6: Write test for builder pattern**

```rust
#[test]
fn test_turn_config_builder() {
    let config = TurnConfigBuilder::default()
        .max_tool_calls(Some(5))
        .tool_timeout_secs(Some(60))
        .max_iterations(Some(20))
        .build()
        .unwrap();
    assert_eq!(config.max_tool_calls, Some(5));
    assert_eq!(config.tool_timeout_secs, Some(60));
    assert_eq!(config.max_iterations, Some(20));
}
```

**Step 7: Run test**

Run: `cargo test -p claw test_turn_config_builder`
Expected: PASS

**Step 8: Add TurnInput type with builder**

```rust
/// Input for a Turn execution.
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

**Step 9: Write test for TurnInput builder**

```rust
#[test]
fn test_turn_input_builder_requires_provider_and_tools() {
    // Provider is required, no default
    let result = TurnInputBuilder::default()
        .messages(vec![])
        .system_prompt("test".to_string())
        .provider(mock_provider)
        .tool_manager(mock_tool_manager)
        .tool_ids(vec!["tool1".to_string()])
        .build();
    assert!(result.is_ok());
}
```

**Step 10: Run test to verify fails (mock types not defined)**

Run: `cargo test -p claw test_turn_input_builder`
Expected: FAIL - mock types not found

**Step 11: Add TurnOutput and TokenUsage types**

```rust
/// Output from a Turn execution.
#[derive(Debug, Clone, Builder)]
#[builder(setter(skip))]
pub struct TurnOutput {
    /// Updated message history (includes assistant response + tool results).
    pub messages: Vec<ChatMessage>,
    /// Token usage statistics.
    #[builder(default)]
    pub token_usage: TokenUsage,
}

/// Token usage statistics.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TokenUsage {
    pub input_tokens: u32,
    pub output_tokens: u32,
    pub total_tokens: u32,
}
```

**Step 12: Run cargo check**

Run: `cargo check -p claw`
Expected: PASS

**Step 13: Commit**

```bash
git add crates/claw/src/agents/turn/config.rs
git commit -m "feat(claw): add turn module config types with derive_builder"
```

---

## Task 3: Error Types
**Files:**
- Create: `crates/claw/src/agents/turn/error.rs`

**Step 1: Write failing test for TurnError**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_turn_error_display() {
        let err = TurnError::ToolNotFound("missing_tool".to_string());
        assert_eq!(err.to_string(), "Tool not found: missing_tool");
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p claw --lib`
Expected: FAIL - TurnError not found

**Step 3: Write minimal implementation**

```rust
use thiserror::Error;
use crate::llm::LlmError;

/// Errors that can occur during Turn execution.
#[derive(Debug, Error)]
pub enum TurnError {
    /// LLM call failed.
    #[error("LLM call failed: {0}")]
    LlmFailed(#[from] LlmError),
    /// Tool not found in registry.
    #[error("Tool not found: {0}")]
    ToolNotFound(String),
    /// Tool execution failed.
    #[error("Tool '{name}' execution failed: {reason}")]
    ToolExecutionFailed { name: String, reason: String },
    /// Tool call blocked by hook.
    #[error("Tool call blocked by hook: {reason}")]
    ToolCallBlocked { reason: String },
    /// Maximum iterations reached.
    #[error("Maximum iterations ({0}) reached")]
    MaxIterationsReached(u32),
    /// Context length exceeded.
    #[error("Context length exceeded: {0} tokens")]
    ContextLengthExceeded(usize),
    /// Turn timeout exceeded.
    #[error("Turn timeout exceeded")]
    TimeoutExceeded,
}
```

**Step 4: Run test to verify it passes**

Run: `cargo test -p claw test_turn_error_display`
Expected: PASS

**Step 5: Write test for error conversion from LlmError**

```rust
#[test]
fn test_turn_error_from_llm_error() {
    let llm_err = LlmError::RequestFailed {
        provider: "test".to_string(),
        reason: "timeout".to_string(),
    };
    let turn_err = TurnError::from(llm_err);
    assert!(matches!(turn_err, TurnError::LlmFailed(_)));
}
```

**Step 6: Run test**

Run: `cargo test -p claw test_turn_error_from_llm_error`
Expected: PASS

**Step 7: Commit**

```bash
git add crates/claw/src/agents/turn/error.rs
git commit -m "feat(claw): add TurnError types"
```

---

## Task 4: Hooks System
**Files:**
- Create: `crates/claw/src/agents/turn/hooks.rs`

**Step 1: Add async_trait and dashmap to Cargo.toml if not present**

Run: `grep -q "async_trait" crates/claw/Cargo.toml`
Expected: If not found, add it.
Run: `grep -q "dashmap" crates/claw/Cargo.toml`
Expected: If not found, add it.

**Step 2: Write failing test for HookRegistry**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    struct TestHandler {
        called: std::sync::Mutex<bool>,
    }

    #[async_trait]
    impl HookHandler for TestHandler {
        async fn on_event(&self, _ctx: &HookContext) -> Result<(), String> {
        *self.called.lock().unwrap().insert(true);
        Ok(())
    }

    #[tokio::test]
    async fn test_hook_registry_fire() {
        let registry = HookRegistry::new();
        let handler = Arc::new(TestHandler {
            called: std::sync::Mutex::new(false),
        });
        registry.register(HookEvent::BeforeToolCall, handler.clone());

        let ctx = HookContext {
            event: HookEvent::BeforeToolCall,
            tool_name: "test".to_string(),
            tool_call_id: "id".to_string(),
            tool_input: serde_json::json!({}),
            tool_result: None,
            error: None,
        };
        registry.fire(&ctx).await.unwrap();
        assert!(*handler.called.lock().unwrap());
    }
}
```

**Step 3: Run test to verify it fails**

Run: `cargo test -p claw --lib`
Expected: FAIL - HookRegistry, HookContext, HookEvent, HookHandler not found

**Step 4: Write minimal implementation**

```rust
use async_trait::async_trait;
use dashmap::DashMap;
use serde_json::Value;
use std::sync::Arc;

/// Hook event types.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookEvent {
    BeforeToolCall,
    AfterToolCall,
    TurnEnd,
}

/// Context passed to hook handlers.
#[derive(Debug, Clone)]
pub struct HookContext {
    /// Which hook event triggered this call.
    pub event: HookEvent,
    /// Tool name being executed.
    pub tool_name: String,
    /// Tool call ID.
    pub tool_call_id: String,
    /// Tool input arguments.
    pub tool_input: Value,
    /// Tool execution result (for AfterToolCall).
    pub tool_result: Option<Value>,
    /// Error message if execution failed.
    pub error: Option<String>,
}

/// Hook handler trait.
#[async_trait]
pub trait HookHandler: Send + Sync {
    /// Handle a hook event.
    ///
    /// For `BeforeToolCall`: returning `Err(reason)` blocks the tool call.
    /// For `AfterToolCall` and `TurnEnd`: return value is ignored (observe-only).
    async fn on_event(&self, ctx: &HookContext) -> Result<(), String>;
}

/// Registry for hook handlers.
#[derive(Default)]
pub struct HookRegistry {
    handlers: DashMap<HookEvent, Vec<Arc<dyn HookHandler>>>,
}

impl HookRegistry {
    pub fn new() -> Self {
        Self {
            handlers: DashMap::new(),
        }
    }

    /// Register a handler for a specific event type.
    pub fn register(&self, event: HookEvent, handler: Arc<dyn HookHandler>) {
        self.handlers.entry(event).or_default().push(handler);
    }

    /// Fire all handlers for an event.
    ///
    /// For `BeforeToolCall`, the first Err stops execution and returns the reason.
    /// For other events, errors are logged but don't propagate.
    pub async fn fire(&self, ctx: &HookContext) -> Result<(), String> {
        if let Some(handlers) = self.handlers.get(&ctx.event) {
            for handler in handlers.iter() {
                if let Err(reason) = handler.on_event(ctx).await {
                    if ctx.event == HookEvent::BeforeToolCall {
                        return Err(reason);
                    }
                    tracing::warn!(
                        event = ?ctx.event,
                        tool_name = %ctx.tool_name,
                        error = %reason,
                        "Hook handler returned error (non-blocking)"
                    );
                }
            }
        }
        Ok(())
    }

    /// Check if any handlers are registered for a given event.
    pub fn has_handlers(&self, event: HookEvent) -> bool {
        self.handlers.get(&event).map(|v| !v.is_empty()).unwrap_or(false)
    }
}
```

**Step 5: Run test to verify it passes**

Run: `cargo test -p claw test_hook_registry_fire`
Expected: PASS

**Step 6: Write test for BeforeToolCall blocking**

```rust
#[tokio::test]
async fn test_hook_before_tool_call_can_block() {
    struct BlockingHandler;

    #[async_trait]
    impl HookHandler for BlockingHandler {
        async fn on_event(&self, _ctx: &HookContext) -> Result<(), String> {
            Err("Tool not allowed".to_string())
        }
    }

    let registry = HookRegistry::new();
    registry.register(HookEvent::BeforeToolCall, Arc::new(BlockingHandler));

    let ctx = HookContext {
        event: HookEvent::BeforeToolCall,
        tool_name: "dangerous_tool".to_string(),
        tool_call_id: "id".to_string(),
        tool_input: serde_json::json!({}),
        tool_result: None,
        error: None,
    };
    let result = registry.fire(&ctx).await;
    assert!(result.is_err());
    assert_eq!(result.unwrap_err(), "Tool not allowed");
}
```

**Step 7: Run test**

Run: `cargo test -p claw test_hook_before_tool_call_can_block`
Expected: PASS

**Step 8: Write test for AfterToolCall observe-only**

```rust
#[tokio::test]
async fn test_hook_after_tool_call_is_observe_only() {
    struct ErrorHandler;

    #[async_trait]
    impl HookHandler for ErrorHandler {
        async fn on_event(&self, _ctx: &HookContext) -> Result<(), String> {
            Err("This error should be ignored".to_string())
        }
    }

    let registry = HookRegistry::new();
    registry.register(HookEvent::AfterToolCall, Arc::new(ErrorHandler));

    let ctx = HookContext {
        event: HookEvent::AfterToolCall,
        tool_name: "test_tool".to_string(),
        tool_call_id: "id".to_string(),
        tool_input: serde_json::json!({}),
        tool_result: Some(serde_json::json!({"result": "ok"})),
        error: None,
    };
    // AfterToolCall is observe-only, error should be swallowed
    let result = registry.fire(&ctx).await;
    assert!(result.is_ok());
}
```

**Step 9: Run test**

Run: `cargo test -p claw test_hook_after_tool_call_is_observe_only`
Expected: PASS

**Step 10: Commit**

```bash
git add crates/claw/src/agents/turn/hooks.rs crates/claw/Cargo.toml
git commit -m "feat(claw): add hooks system with HookRegistry"
```

---

## Task 5: Core Execution Logic
**Files:**
- Create: `crates/claw/src/agents/turn/execution.rs`

**Step 1: Write failing test for execute_turn with empty response**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm::{LlmProvider, CompletionResponse, FinishReason, LlmError};
    use crate::tool::ToolManager;
    use async_trait::async_trait;
    use rust_decimal::Decimal;
    use std::sync::Arc;

    struct MockProvider {
        response: CompletionResponse,
    }

    #[async_trait]
    impl LlmProvider for MockProvider {
        fn model_name(&self) -> &str { "mock" }
        fn cost_per_token(&self) -> (Decimal, Decimal) { (Decimal::ZERO, Decimal::ZERO) }
        async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
            Ok(self.response.clone())
        }
        async fn complete_with_tools(&self, _request: ToolCompletionRequest) -> Result<ToolCompletionResponse, LlmError> {
            unimplemented!()
        }
    }

    #[tokio::test]
    async fn test_execute_turn_returns_final_response() {
        let provider = Arc::new(MockProvider {
            response: CompletionResponse {
                content: Some("Hello, world!".to_string()),
                reasoning_content: None,
                input_tokens: 10,
                output_tokens: 5,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
        });

        let tool_manager = Arc::new(ToolManager::new());
        let input = TurnInputBuilder::default()
            .messages(vec![])
            .system_prompt("You are helpful.".to_string())
            .provider(provider)
            .tool_manager(tool_manager)
            .tool_ids(vec![])
            .build()
            .unwrap();

        let config = TurnConfig::new();
        let result = execute_turn(input, config).await.unwrap();

        assert_eq!(result.messages.len(), 1);
        assert_eq!(result.messages[0].role, Role::Assistant);
        assert_eq!(result.messages[0].content, "Hello, world!");
        assert_eq!(result.token_usage.input_tokens, 10);
        assert_eq!(result.token_usage.output_tokens, 5);
    }
}
```

**Step 2: Run test to verify it fails**

Run: `cargo test -p claw test_execute_turn_returns_final_response`
Expected: FAIL - execute_turn not found

**Step 3: Write minimal execute_turn skeleton**

```rust
use std::sync::Arc;
use crate::llm::{ChatMessage, LlmProvider, ToolCompletionRequest, ToolCompletionResponse, ToolCall, ToolDefinition, FinishReason};
use crate::tool::ToolManager;
use super::{TurnConfig, TurnInput, TurnOutput, TurnError, TokenUsage};
use super::hooks::{HookContext, HookEvent, HookRegistry};

/// Execute a single Turn.
pub async fn execute_turn(
    input: TurnInput,
    config: TurnConfig,
) -> Result<TurnOutput, TurnError> {
    todo!("implement turn execution")
}
```

**Step 4: Run test to see exact failure**

Run: `cargo test -p claw test_execute_turn_returns_final_response`
Expected: FAIL - todo! panic

**Step 5: Implement basic non-tool response path**

```rust
pub async fn execute_turn(
    input: TurnInput,
    config: TurnConfig,
) -> Result<TurnOutput, TurnError> {
    let mut messages = input.messages.clone();
    let tool_manager = &input.tool_manager;
    let tool_ids = &input.tool_ids;

    // Resolve tools from tool_manager
    let tools: Vec<ToolDefinition> = tool_ids
        .iter()
        .filter_map(|id| tool_manager.get(id))
        .map(|tool| tool.definition())
        .collect();

    let mut total_usage = TokenUsage::default();
    let max_iterations = config.max_iterations.unwrap_or(50);

    // Single iteration for now
    let request = ToolCompletionRequest::new(
        messages.clone(),
        tools.clone(),
    )
    .with_model(input.provider.model_name().to_string());

    let response = input.provider.complete_with_tools(request).await
        .map_err(|e| TurnError::LlmFailed(e))?;

    total_usage.input_tokens += response.input_tokens;
    total_usage.output_tokens += response.output_tokens;

    match response.finish_reason {
        FinishReason::Stop => {
            messages.push(ChatMessage::assistant(
                response.content.unwrap_or_default()
            ));
            Ok(TurnOutput {
                messages,
                token_usage: total_usage,
            })
        }
        _ => todo!("handle other finish reasons"),
    }
}
```

**Step 6: Run test**

Run: `cargo test -p claw test_execute_turn_returns_final_response`
Expected: PASS

**Step 7: Write test for tool use path**

```rust
#[tokio::test]
async fn test_execute_turn_with_tool_call() {
    use crate::tool::{NamedTool, ToolError};
    use crate::llm::ToolDefinition;

    struct EchoTool;

    #[async_trait]
    impl NamedTool for EchoTool {
        fn name(&self) -> &str { "echo" }
        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "echo".to_string(),
                description: "Echoes input".to_string(),
                parameters: serde_json::json!({"type": "object"}),
            }
        }
        async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value, ToolError> {
            Ok(args)
        }
    }

    let tool_manager = Arc::new(ToolManager::new());
    tool_manager.register(Arc::new(EchoTool));

    // First response: tool call. Second response: final answer.
    let responses = vec![
        ToolCompletionResponse {
            content: None,
            reasoning_content: None,
            tool_calls: vec![ToolCall {
                id: "call_1".to_string(),
                name: "echo".to_string(),
                arguments: serde_json::json!({"message": "hello"}),
            }],
            input_tokens: 10,
            output_tokens: 5,
            finish_reason: FinishReason::ToolUse,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        },
        ToolCompletionResponse {
            content: Some("Done!".to_string()),
            reasoning_content: None,
            tool_calls: vec![],
            input_tokens: 15,
            output_tokens: 3,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        },
    ];

    let provider = Arc::new(SequentialMockProvider { responses, call_count: std::sync::Mutex::new(0) });
    // ... rest of test
}
```

**Step 8: Implement tool execution loop**

(This is a more complex step - implement the full loop with parallel tool execution)

**Step 9-15: Continue implementing and testing each path**

(Details omitted for brevity - see full implementation)

**Step 16: Commit**

```bash
git add crates/claw/src/agents/turn/execution.rs
git commit -m "feat(claw): implement turn execution with parallel tool support"
```

---

## Task 6: Module Re-exports and Final Tests
**Files:**
- Modify: `crates/claw/src/agents/turn/mod.rs`
- Modify: `crates/claw/src/lib.rs`

**Step 1: Update turn/mod.rs to re-export execution**

```rust
mod config;
mod error;
mod execution;
mod hooks;

pub use config::{TurnConfig, TurnConfigBuilder, TurnInput, TurnInputBuilder, TurnOutput, TurnOutputBuilder, TokenUsage};
pub use error::TurnError;
pub use execution::execute_turn;
pub use hooks::{HookContext, HookEvent, HookHandler, HookRegistry};
```

**Step 2: Update lib.rs to export turn module**

```rust
pub mod agents {
    pub mod turn;
}
```

**Step 3: Run all tests**

Run: `cargo test -p claw`
Expected: All tests pass

**Step 4: Run clippy**

Run: `cargo clippy -p claw --all-targets -- -D warnings`
Expected: No warnings

**Step 5: Commit**

```bash
git add crates/claw/src/agents/turn/mod.rs crates/claw/src/lib.rs
git commit -m "feat(claw): export turn module from agents"
```

---

## Task 7: Integration Test
**Files:**
- Create: `crates/claw/tests/turn_integration_test.rs`

**Step 1: Write integration test with real ToolManager**

Test end-to-end turn execution with multiple tool calls.

**Step 2: Run test**

Run: `cargo test -p claw --test turn_integration_test`
Expected: PASS

**Step 3: Commit**

```bash
git add crates/claw/tests/turn_integration_test.rs
git commit -m "test(claw): add turn module integration test"
```

---

## Summary

- **Files created:** 5 (mod.rs, config.rs, error.rs, hooks.rs, execution.rs)
- **Files modified:** 2 (agents/mod.rs, lib.rs)
- **Test files:** 2 (unit tests in each module, 1 integration test)
- **Total tasks:** 7
- **Estimated time:** 2-3 hours with TDD approach
