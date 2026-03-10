use async_trait::async_trait;
use dashmap::DashMap;
use serde_json::Value;
use std::sync::Arc;

/// Hook event types that can be intercepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HookEvent {
    /// Fires before a tool call. Handler can block by returning Err.
    BeforeToolCall,
    /// Fires after a tool call completes. Observe-only.
    AfterToolCall,
    /// Fires after the turn completes. Observe-only.
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

/// Hook handler trait for intercepting Turn events.
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
        self.handlers
            .get(&event)
            .map(|v| !v.is_empty())
            .unwrap_or(false)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestHandler {
        called: std::sync::Mutex<bool>,
    }

    #[async_trait]
    impl HookHandler for TestHandler {
        async fn on_event(&self, _ctx: &HookContext) -> Result<(), String> {
            *self.called.lock().unwrap() = true;
            Ok(())
        }
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
}
