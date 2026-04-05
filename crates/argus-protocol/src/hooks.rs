//! Hook system for intercepting Turn execution.
//!
//! Hooks can intercept tool execution and turn completion:
//! - `BeforeToolCall`: Can block tool execution
//! - `AfterToolCall`: Observe tool results
//! - `TurnEnd`: Observe turn completion and optionally request loop continuation

use async_trait::async_trait;
use dashmap::DashMap;
use serde_json::Value;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Hook event types that can be intercepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookEvent {
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
    /// Continue the turn loop with an injected user message (TurnEnd only).
    ContinueWithMessage(String),
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
    /// Thread event sender for broadcasting thread events (optional).
    pub thread_event_sender: Option<broadcast::Sender<crate::events::ThreadEvent>>,
    /// Thread ID for event context (optional).
    pub thread_id: Option<String>,
    /// Turn number for event context.
    pub turn_number: Option<u32>,
}

/// Hook handler trait for intercepting Turn events.
#[async_trait]
pub trait HookHandler: Send + Sync {
    /// Handle tool-related events (BeforeToolCall, AfterToolCall, TurnEnd).
    ///
    /// For `BeforeToolCall`: returning `Block(reason)` prevents tool execution.
    /// For `TurnEnd`: `ContinueWithMessage` can be interpreted by turn executors.
    /// For other events: return value is observe-only.
    async fn on_tool_event(&self, _ctx: &ToolHookContext) -> HookAction {
        HookAction::Continue
    }
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

    /// Fire tool-related hooks.
    ///
    /// For `BeforeToolCall`, the first `Block` stops execution and returns the reason.
    /// For other events, errors are logged but don't propagate.
    /// `ContinueWithMessage` is logged and ignored in registry path.
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
                HookAction::ContinueWithMessage(message) => {
                    if matches!(ctx.event, HookEvent::TurnEnd) {
                        tracing::debug!(
                            event = ?ctx.event,
                            message = %message,
                            "Hook handler returned ContinueWithMessage (registry path is observe-only, ignored)"
                        );
                    } else {
                        tracing::warn!(
                            event = ?ctx.event,
                            message = %message,
                            "Hook handler returned ContinueWithMessage on non-TurnEnd event (ignored)"
                        );
                    }
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
    async fn continue_with_message_is_ignored_on_before_tool_call() {
        struct ContinueMessageHandler;

        #[async_trait]
        impl HookHandler for ContinueMessageHandler {
            async fn on_tool_event(&self, _ctx: &ToolHookContext) -> HookAction {
                HookAction::ContinueWithMessage("continue".to_string())
            }
        }

        let registry = HookRegistry::new();
        registry.register(HookEvent::BeforeToolCall, Arc::new(ContinueMessageHandler));

        let ctx = ToolHookContext {
            event: HookEvent::BeforeToolCall,
            tool_name: "test_tool".to_string(),
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
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn continue_with_message_is_ignored_on_after_tool_call() {
        struct ContinueMessageHandler;

        #[async_trait]
        impl HookHandler for ContinueMessageHandler {
            async fn on_tool_event(&self, _ctx: &ToolHookContext) -> HookAction {
                HookAction::ContinueWithMessage("continue".to_string())
            }
        }

        let registry = HookRegistry::new();
        registry.register(HookEvent::AfterToolCall, Arc::new(ContinueMessageHandler));

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

        let result = registry.fire_tool_event(&ctx).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn continue_with_message_is_accepted_on_turn_end_registry_path() {
        struct ContinueMessageHandler;

        #[async_trait]
        impl HookHandler for ContinueMessageHandler {
            async fn on_tool_event(&self, _ctx: &ToolHookContext) -> HookAction {
                HookAction::ContinueWithMessage("continue".to_string())
            }
        }

        let registry = HookRegistry::new();
        registry.register(HookEvent::TurnEnd, Arc::new(ContinueMessageHandler));

        let ctx = ToolHookContext {
            event: HookEvent::TurnEnd,
            tool_name: String::new(),
            tool_call_id: String::new(),
            tool_input: serde_json::Value::Null,
            tool_result: None,
            error: None,
            tool_manager: None,
            thread_event_sender: None,
            thread_id: None,
            turn_number: None,
        };

        let result = registry.fire_tool_event(&ctx).await;
        assert!(result.is_ok());
    }
}
