//! Approval hook - integrates with Turn execution through the hook system
//!
//! This hook:
//! - Checks if a tool requires approval based on the current policy
//! - If so, creates an approval request and waits for approval/denied/timeout
//! - Returns `HookAction::Continue` or `Block(reason)` based on the decision
//!
//! # Example
//!
//! ```ignore
//! use std::sync::Arc;
//! use claw::approval::{ApprovalManager, ApprovalPolicy, ApprovalHook};
//! use claw::protocol::{HookEvent, HookRegistry};
//!
//! let policy = ApprovalPolicy::default();
//! let manager = Arc::new(ApprovalManager::new(policy.clone()));
//! let hook = ApprovalHook::new(manager, policy, "agent-1");
//!
//! // Register with HookRegistry
//! let registry = HookRegistry::new();
//! registry.register(HookEvent::BeforeToolCall, Arc::new(hook));
//! ```

use async_trait::async_trait;
use std::sync::Arc;

use crate::approval::{ApprovalDecision, ApprovalManager, ApprovalPolicy, ApprovalRequest};
use crate::protocol::{HookAction, HookEvent, HookHandler, RiskLevel, ToolHookContext};

/// Approval hook that integrates with Turn execution through the hook system.
///
/// This hook checks if a tool requires approval before execution. If approval
/// is required, it creates an approval request and waits for a decision.
pub struct ApprovalHook {
    /// The approval manager instance.
    approval_manager: Arc<ApprovalManager>,
    /// The policy determining which tools require approval.
    policy: ApprovalPolicy,
    /// Agent ID for approval requests.
    agent_id: String,
}

impl ApprovalHook {
    /// Create a new approval hook.
    pub fn new(
        approval_manager: Arc<ApprovalManager>,
        policy: ApprovalPolicy,
        agent_id: impl Into<String>,
    ) -> Self {
        Self {
            approval_manager,
            policy,
            agent_id: agent_id.into(),
        }
    }
}

#[async_trait]
impl HookHandler for ApprovalHook {
    async fn on_tool_event(&self, ctx: &ToolHookContext) -> HookAction {
        // Only intercept BeforeToolCall events
        if ctx.event != HookEvent::BeforeToolCall {
            return HookAction::Continue;
        }

        // Check if this tool requires approval based on policy
        if !self.policy.requires_approval(&ctx.tool_name) {
            return HookAction::Continue;
        }

        // Get risk level from tool_manager if available
        let risk_level = ctx
            .tool_manager
            .as_ref()
            .map(|tm| tm.get_risk_level(&ctx.tool_name))
            .unwrap_or(RiskLevel::Low);

        // Create approval request
        let action_summary = format!("Execute: {}", ctx.tool_input);
        let req = ApprovalRequest::new(
            self.agent_id.clone(),
            ctx.tool_name.clone(),
            action_summary,
            self.policy.timeout_secs,
            risk_level,
        );

        // Request approval (this blocks until approved/denied/timeout)
        let decision = self.approval_manager.request_approval(req).await;

        // Return appropriate action based on decision
        match decision {
            ApprovalDecision::Approved => HookAction::Continue,
            ApprovalDecision::Denied => HookAction::Block(format!(
                "Approval denied for tool '{}': rejected by operator",
                ctx.tool_name
            )),
            ApprovalDecision::TimedOut => HookAction::Block(format!(
                "Approval timed out for tool '{}' after {}s",
                ctx.tool_name, self.policy.timeout_secs
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::HookEvent;

    fn create_test_policy() -> ApprovalPolicy {
        ApprovalPolicy {
            require_approval: vec!["shell".to_string()],
            timeout_secs: 60,
            auto_approve: false,
            auto_approve_autonomous: false,
        }
    }

    #[tokio::test]
    async fn test_approval_hook_skips_non_before_tool_call_events() {
        let policy = create_test_policy();
        let manager = Arc::new(ApprovalManager::new(policy.clone()));
        let hook = ApprovalHook::new(manager, policy, "test-agent");

        let ctx = ToolHookContext {
            event: HookEvent::AfterToolCall,
            tool_name: "shell".to_string(),
            tool_call_id: "test-id".to_string(),
            tool_input: serde_json::json!({"command": "echo test"}),
            tool_result: None,
            error: None,
            tool_manager: None,
        };

        let action = hook.on_tool_event(&ctx).await;
        assert!(matches!(action, HookAction::Continue));
    }

    #[tokio::test]
    async fn test_approval_hook_skips_tools_not_in_policy() {
        let policy = create_test_policy();
        let manager = Arc::new(ApprovalManager::new(policy.clone()));
        let hook = ApprovalHook::new(manager, policy, "test-agent");

        let ctx = ToolHookContext {
            event: HookEvent::BeforeToolCall,
            tool_name: "read_file".to_string(),
            tool_call_id: "test-id".to_string(),
            tool_input: serde_json::json!({"path": "/tmp/test.txt"}),
            tool_result: None,
            error: None,
            tool_manager: None,
        };

        let action = hook.on_tool_event(&ctx).await;
        assert!(matches!(action, HookAction::Continue));
    }
}
