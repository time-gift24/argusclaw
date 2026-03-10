// Hook system for turn lifecycle events
// Types: HookContext, HookEvent, HookHandler, HookRegistry

use std::sync::Arc;

/// Context provided to hook handlers during execution.
#[derive(Debug, Clone)]
pub struct HookContext {
    /// Turn identifier for tracing.
    pub turn_id: String,
}

/// Events that can trigger hooks during turn execution.
#[derive(Debug, Clone)]
pub enum HookEvent {
    /// Turn execution started.
    TurnStarted,
    /// About to call LLM.
    BeforeLlmCall,
    /// LLM call completed.
    AfterLlmCall,
    /// About to execute a tool.
    BeforeToolCall {
        /// Tool name.
        tool_name: String,
        /// Tool call ID.
        call_id: String,
    },
    /// Tool execution completed.
    AfterToolCall {
        /// Tool name.
        tool_name: String,
        /// Tool call ID.
        call_id: String,
    },
    /// Turn execution completed.
    TurnCompleted,
    /// Turn execution failed.
    TurnFailed {
        /// Error message.
        error: String,
    },
}

/// Trait for hook handlers.
pub trait HookHandler: Send + Sync {
    /// Handle a hook event.
    fn handle(&self, event: &HookEvent, context: &HookContext);
}

/// Registry for hook handlers.
#[derive(Default)]
pub struct HookRegistry {
    handlers: Vec<Arc<dyn HookHandler>>,
}

impl std::fmt::Debug for HookRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HookRegistry")
            .field("handler_count", &self.handlers.len())
            .finish()
    }
}

impl HookRegistry {
    /// Create a new empty hook registry.
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
        }
    }

    /// Register a hook handler.
    pub fn register(&mut self, handler: Arc<dyn HookHandler>) {
        self.handlers.push(handler);
    }

    /// Trigger an event to all registered handlers.
    pub fn trigger(&self, event: &HookEvent, context: &HookContext) {
        for handler in &self.handlers {
            handler.handle(event, context);
        }
    }
}
