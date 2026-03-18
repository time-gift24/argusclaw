//! Hook system for intercepting and modifying Turn execution.
//!
//! Hooks allow intercepting and potentially modifying the execution flow:
//! - `BeforeCallLLM`: Can modify messages and tools before each LLM call
//! - `BeforeToolCall`: Can block tool execution
//! - `AfterToolCall`: Observe tool results
//! - `TurnEnd`: Observe turn completion

use async_trait::async_trait;
use dashmap::DashMap;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::broadcast;

use crate::llm::{ChatMessage, ToolDefinition};

/// Hook event types that can be intercepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookEvent {
    /// Fires before calling the LLM. Handler can modify messages/tools or block.
    BeforeCallLLM,
    /// Fires before a tool call. Handler can block by returning `Block`.
    BeforeToolCall,
    /// Fires after a tool call completes. Observe-only.
    AfterToolCall,
    /// Fires after the turn completes. Observe-only.
    TurnEnd,
}

/// Action returned by hook handlers.
#[derive(Debug, Default)]
pub enum HookAction {
    /// Continue with no modifications.
    #[default]
    Continue,
    /// Block execution with a reason.
    Block(String),
    /// Modify messages before calling LLM (only effective for BeforeCallLLM).
    ModifyMessages(Vec<ChatMessage>),
    /// Modify tools before calling LLM (only effective for BeforeCallLLM).
    ModifyTools(Vec<ToolDefinition>),
    /// Modify both messages and tools (only effective for BeforeCallLLM).
    Modify {
        messages: Vec<ChatMessage>,
        tools: Vec<ToolDefinition>,
    },
}

/// Context for BeforeCallLLM hook - allows access to messages and tools.
#[derive(Debug, Clone)]
pub struct BeforeCallLLMContext {
    /// Current messages that will be sent to LLM.
    pub messages: Vec<ChatMessage>,
    /// Current tools available to the LLM.
    pub tools: Vec<ToolDefinition>,
    /// Number of iterations completed so far.
    pub iteration: u32,
}

/// Context passed to hook handlers for tool-related events.
#[derive(Clone)]
pub struct ToolHookContext {
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
    /// Tool manager for accessing tool metadata (e.g., risk level).
    pub tool_manager: Option<Arc<dyn crate::tool::NamedTool>>,
    /// Thread event sender for broadcasting approval events (optional).
    pub thread_event_sender: Option<broadcast::Sender<crate::events::ThreadEvent>>,
    /// Thread ID for event context (optional).
    pub thread_id: Option<String>,
    /// Turn number for event context.
    pub turn_number: Option<u32>,
}

/// Hook handler trait for intercepting Turn events.
#[async_trait]
pub trait HookHandler: Send + Sync {
    /// Handle BeforeCallLLM event.
    ///
    /// Can modify messages and tools by returning appropriate `HookAction`.
    /// Return `Block(reason)` to prevent the LLM call.
    async fn on_before_call_llm(&self, _ctx: &BeforeCallLLMContext) -> HookAction {
        HookAction::Continue
    }

    /// Handle tool-related events (BeforeToolCall, AfterToolCall, TurnEnd).
    ///
    /// For `BeforeToolCall`: returning `Block(reason)` prevents tool execution.
    /// For other events: return value is ignored (observe-only).
    async fn on_tool_event(&self, _ctx: &ToolHookContext) -> HookAction {
        HookAction::Continue
    }
}

/// Result of firing BeforeCallLLM hooks.
#[derive(Debug, Default)]
pub struct BeforeCallLLMResult {
    /// Modified messages (if any handler modified them).
    pub messages: Option<Vec<ChatMessage>>,
    /// Modified tools (if any handler modified them).
    pub tools: Option<Vec<ToolDefinition>>,
}

/// Union type for hook contexts.
///
/// This enum replaces `&dyn Any` to ensure `Send + Sync` bounds are satisfied
/// when hooks are fired across async boundaries.
#[derive(Clone)]
pub enum HookContext {
    /// Context for BeforeCallLLM events.
    BeforeCallLLM(BeforeCallLLMContext),
    /// Context for tool-related events (BeforeToolCall, AfterToolCall, TurnEnd).
    ToolEvent(ToolHookContext),
}

/// Registry for hook handlers.
#[derive(Default)]
pub struct HookRegistry {
    handlers: DashMap<HookEvent, Vec<Arc<dyn HookHandler>>>,
}

impl HookRegistry {
    /// Create a new empty registry.
    pub fn new() -> Self {
        Self {
            handlers: DashMap::new(),
        }
    }

    /// Register a handler for a specific event type.
    pub fn register(&self, event: HookEvent, handler: Arc<dyn HookHandler>) {
        self.handlers.entry(event).or_default().push(handler);
    }

    /// Fire BeforeCallLLM hooks.
    ///
    /// Handlers can modify messages and tools. The first `Block` stops execution.
    /// Modifications are cumulative - each handler sees the result of previous handlers.
    pub async fn fire_before_call_llm(
        &self,
        ctx: &BeforeCallLLMContext,
    ) -> Result<BeforeCallLLMResult, String> {
        let Some(handlers) = self.handlers.get(&HookEvent::BeforeCallLLM) else {
            return Ok(BeforeCallLLMResult::default());
        };

        let mut result = BeforeCallLLMResult::default();
        let mut current_messages = ctx.messages.clone();
        let mut current_tools = ctx.tools.clone();

        for handler in handlers.iter() {
            let ctx = BeforeCallLLMContext {
                messages: current_messages.clone(),
                tools: current_tools.clone(),
                iteration: ctx.iteration,
            };

            match handler.on_before_call_llm(&ctx).await {
                HookAction::Continue => {}
                HookAction::Block(reason) => return Err(reason),
                HookAction::ModifyMessages(messages) => {
                    result.messages = Some(messages.clone());
                    current_messages = messages;
                }
                HookAction::ModifyTools(tools) => {
                    result.tools = Some(tools.clone());
                    current_tools = tools;
                }
                HookAction::Modify { messages, tools } => {
                    result.messages = Some(messages.clone());
                    result.tools = Some(tools.clone());
                    current_messages = messages;
                    current_tools = tools;
                }
            }
        }

        Ok(result)
    }

    /// Fire tool-related hooks.
    ///
    /// For `BeforeToolCall`, the first `Block` stops execution and returns the reason.
    /// For other events, errors are logged but don't propagate.
    pub async fn fire_tool_event(&self, ctx: &ToolHookContext) -> Result<(), String> {
        let Some(handlers) = self.handlers.get(&ctx.event) else {
            return Ok(());
        };

        for handler in handlers.iter() {
            match handler.on_tool_event(ctx).await {
                HookAction::Continue => {}
                HookAction::Block(reason) => {
                    if matches!(ctx.event, HookEvent::BeforeToolCall) {
                        return Err(reason);
                    }
                    tracing::warn!(
                        event = ?ctx.event,
                        tool_name = %ctx.tool_name,
                        error = %reason,
                        "Hook handler returned Block (non-blocking event)"
                    );
                }
                _ => {
                    tracing::warn!(
                        event = ?ctx.event,
                        "Hook handler returned modification action on non-modifiable event (ignored)"
                    );
                }
            }
        }

        Ok(())
    }

    /// Check if any handlers are registered for a given event.
    pub fn has_handlers(&self, event: HookEvent) -> bool {
        self.handlers
            .get(&event)
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    }

    /// Get all registered handlers across all events.
    ///
    /// This is useful when migrating from HookRegistry to direct handler ownership
    /// (e.g., when building a Turn with tools and hooks).
    pub fn all_handlers(&self) -> Vec<Arc<dyn HookHandler>> {
        let mut all = Vec::new();
        for entry in self.handlers.iter() {
            for handler in entry.value().iter() {
                all.push(handler.clone());
            }
        }
        all
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestHandler;

    #[async_trait]
    impl HookHandler for TestHandler {
        async fn on_tool_event(&self, _ctx: &ToolHookContext) -> HookAction {
            HookAction::Continue
        }
    }

    #[tokio::test]
    async fn test_hook_registry_fire_before_call_llm() {
        let registry = HookRegistry::new();
        registry.register(HookEvent::BeforeCallLLM, Arc::new(TestHandler));

        let ctx = BeforeCallLLMContext {
            messages: vec![ChatMessage::user("Hello")],
            tools: vec![],
            iteration: 0,
        };
        let result = registry.fire_before_call_llm(&ctx).await.unwrap();
        assert!(result.messages.is_none());
        assert!(result.tools.is_none());
    }

    #[tokio::test]
    async fn test_hook_before_call_llm_can_modify_messages() {
        struct ModifyHandler;

        #[async_trait]
        impl HookHandler for ModifyHandler {
            async fn on_before_call_llm(&self, ctx: &BeforeCallLLMContext) -> HookAction {
                let mut messages = ctx.messages.clone();
                messages.push(ChatMessage::system("Be helpful"));
                HookAction::ModifyMessages(messages)
            }
        }

        let registry = HookRegistry::new();
        registry.register(HookEvent::BeforeCallLLM, Arc::new(ModifyHandler));

        let ctx = BeforeCallLLMContext {
            messages: vec![ChatMessage::user("Hello")],
            tools: vec![],
            iteration: 0,
        };
        let result = registry.fire_before_call_llm(&ctx).await.unwrap();
        assert!(result.messages.is_some());
        let messages = result.messages.unwrap();
        assert_eq!(messages.len(), 2);
        assert_eq!(messages[1].role, crate::llm::Role::System);
    }

    #[tokio::test]
    async fn test_hook_before_call_llm_can_block() {
        struct BlockingHandler;

        #[async_trait]
        impl HookHandler for BlockingHandler {
            async fn on_before_call_llm(&self, _ctx: &BeforeCallLLMContext) -> HookAction {
                HookAction::Block("Rate limit exceeded".to_string())
            }
        }

        let registry = HookRegistry::new();
        registry.register(HookEvent::BeforeCallLLM, Arc::new(BlockingHandler));

        let ctx = BeforeCallLLMContext {
            messages: vec![],
            tools: vec![],
            iteration: 0,
        };
        let result = registry.fire_before_call_llm(&ctx).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Rate limit exceeded");
    }

    #[tokio::test]
    async fn test_hook_before_tool_call_can_block() {
        struct BlockingHandler;

        #[async_trait]
        impl HookHandler for BlockingHandler {
            async fn on_tool_event(&self, _ctx: &ToolHookContext) -> HookAction {
                HookAction::Block("Tool not allowed".to_string())
            }
        }

        let registry = HookRegistry::new();
        registry.register(HookEvent::BeforeToolCall, Arc::new(BlockingHandler));

        let ctx = ToolHookContext {
            event: HookEvent::BeforeToolCall,
            tool_name: "dangerous_tool".to_string(),
            tool_call_id: "id".to_string(),
            tool_input: serde_json::json!({}),
            tool_result: None,
            error: None,
            tool_manager: None,
            thread_event_sender: None,
            thread_id: None,
            turn_number: None,
        };
        let result = registry.fire_tool_event(&ctx).await;
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), "Tool not allowed");
    }

    #[tokio::test]
    async fn test_hook_after_tool_call_is_observe_only() {
        struct ErrorHandler;

        #[async_trait]
        impl HookHandler for ErrorHandler {
            async fn on_tool_event(&self, _ctx: &ToolHookContext) -> HookAction {
                HookAction::Block("This should be ignored".to_string())
            }
        }

        let registry = HookRegistry::new();
        registry.register(HookEvent::AfterToolCall, Arc::new(ErrorHandler));

        let ctx = ToolHookContext {
            event: HookEvent::AfterToolCall,
            tool_name: "test_tool".to_string(),
            tool_call_id: "id".to_string(),
            tool_input: serde_json::json!({}),
            tool_result: Some(serde_json::json!({"result": "ok"})),
            error: None,
            tool_manager: None,
            thread_event_sender: None,
            thread_id: None,
            turn_number: None,
        };
        // AfterToolCall is observe-only, Block should be swallowed
        let result = registry.fire_tool_event(&ctx).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_cumulative_modifications() {
        struct AddSystemHandler;
        struct AddUserHandler;

        #[async_trait]
        impl HookHandler for AddSystemHandler {
            async fn on_before_call_llm(&self, ctx: &BeforeCallLLMContext) -> HookAction {
                let mut messages = ctx.messages.clone();
                messages.insert(0, ChatMessage::system("System prompt"));
                HookAction::ModifyMessages(messages)
            }
        }

        #[async_trait]
        impl HookHandler for AddUserHandler {
            async fn on_before_call_llm(&self, ctx: &BeforeCallLLMContext) -> HookAction {
                let mut messages = ctx.messages.clone();
                messages.push(ChatMessage::user("Additional question"));
                HookAction::ModifyMessages(messages)
            }
        }

        let registry = HookRegistry::new();
        registry.register(HookEvent::BeforeCallLLM, Arc::new(AddSystemHandler));
        registry.register(HookEvent::BeforeCallLLM, Arc::new(AddUserHandler));

        let ctx = BeforeCallLLMContext {
            messages: vec![ChatMessage::user("Original question")],
            tools: vec![],
            iteration: 0,
        };
        let result = registry.fire_before_call_llm(&ctx).await.unwrap();
        let messages = result.messages.unwrap();
        assert_eq!(messages.len(), 3);
        assert_eq!(messages[0].role, crate::llm::Role::System);
        assert_eq!(messages[1].role, crate::llm::Role::User);
        assert!(messages[1].content.contains("Original"));
        assert_eq!(messages[2].role, crate::llm::Role::User);
        assert!(messages[2].content.contains("Additional"));
    }
}
