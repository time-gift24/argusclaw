//! Approval hook - integrates with Turn execution through the hook system.
//!
//! This hook:
//! - Checks the runtime allow list first (tools user has already approved)
//! - Checks if a tool requires approval based on the current policy
//! - If so, creates an approval request and waits for approval/denied/timeout
//! - Returns `HookAction::Continue` or `Block(reason)` based on the decision
//! - Sends ThreadEvents (WaitingForApproval, ApprovalResolved) to notify frontend
//!
//! # Example
//!
//! ```ignore
//! use std::sync::{Arc, RwLock};
//! use argus_approval::{ApprovalManager, ApprovalPolicy, ApprovalHook, RuntimeAllowList};
//! use argus_protocol::{HookEvent, HookRegistry};
//!
//! let policy = ApprovalPolicy::default();
//! let manager = Arc::new(ApprovalManager::new(policy.clone()));
//! let allow_list = Arc::new(RwLock::new(RuntimeAllowList::new()));
//! let hook = ApprovalHook::new(manager, policy, allow_list, "agent-1");
//!
//! // Register with HookRegistry
//! let registry = HookRegistry::new();
//! registry.register(HookEvent::BeforeToolCall, Arc::new(hook));
//! ```

use async_trait::async_trait;
use chrono::Utc;
use std::sync::{Arc, RwLock};

use argus_protocol::{
    ApprovalDecision, ApprovalRequest, ApprovalResponse, HookAction, HookEvent, HookHandler,
    RiskLevel, ThreadEvent, ToolHookContext,
};

use super::manager::ApprovalManager;
use super::policy::ApprovalPolicy;
use super::runtime_allow::RuntimeAllowList;

/// Approval hook that integrates with Turn execution through the hook system.
///
/// This hook checks if a tool requires approval before execution. If approval
/// is required, it creates an approval request and waits for a decision.
pub struct ApprovalHook {
    /// The approval manager instance.
    approval_manager: Arc<ApprovalManager>,
    /// The policy determining which tools require approval.
    policy: ApprovalPolicy,
    /// Runtime allow list - tools that have been marked as allowed.
    runtime_allow: Arc<RwLock<RuntimeAllowList>>,
    /// Agent ID for approval requests.
    agent_id: String,
}

impl ApprovalHook {
    /// Create a new approval hook.
    pub fn new(
        approval_manager: Arc<ApprovalManager>,
        policy: ApprovalPolicy,
        runtime_allow: Arc<RwLock<RuntimeAllowList>>,
        agent_id: impl Into<String>,
    ) -> Self {
        Self {
            approval_manager,
            policy,
            runtime_allow,
            agent_id: agent_id.into(),
        }
    }

    /// Get a reference to the runtime allow list.
    pub fn runtime_allow(&self) -> &Arc<RwLock<RuntimeAllowList>> {
        &self.runtime_allow
    }
}

#[async_trait]
impl HookHandler for ApprovalHook {
    async fn on_tool_event(&self, ctx: &ToolHookContext) -> HookAction {
        // Only intercept BeforeToolCall events
        if ctx.event != HookEvent::BeforeToolCall {
            return HookAction::Continue;
        }

        // First check runtime allow list (user has already approved this tool)
        if self
            .runtime_allow
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .is_allowed(&ctx.tool_name)
        {
            return HookAction::Continue;
        }

        // Then check policy
        if !self.policy.requires_approval(&ctx.tool_name) {
            return HookAction::Continue;
        }

        // Get risk level from tool if available
        let risk_level = ctx
            .tool_manager
            .as_ref()
            .map(|tool| tool.risk_level())
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

        // Send WaitingForApproval event to thread subscribers
        if let (Some(sender), Some(thread_id), Some(turn_number)) = (
            &ctx.thread_event_sender,
            ctx.thread_id.clone(),
            ctx.turn_number,
        ) {
            let _ = sender.send(ThreadEvent::WaitingForApproval {
                thread_id,
                turn_number,
                request: req.clone(),
            });
        }

        // Request approval (this blocks until approved/denied/timeout)
        let decision = self.approval_manager.request_approval(req.clone()).await;

        // If approved for session, enable allow_all for all future tool calls
        if matches!(decision, ApprovalDecision::ApprovedSession) {
            self.runtime_allow
                .write()
                .unwrap_or_else(|e| e.into_inner())
                .allow_all();
        }

        // Send ApprovalResolved event to thread subscribers
        if let (Some(sender), Some(thread_id), Some(turn_number)) = (
            &ctx.thread_event_sender,
            ctx.thread_id.clone(),
            ctx.turn_number,
        ) {
            let response = ApprovalResponse {
                request_id: req.id,
                decision,
                decided_at: Utc::now(),
                decided_by: None,
            };
            let _ = sender.send(ThreadEvent::ApprovalResolved {
                thread_id,
                turn_number,
                response,
            });
        }

        // Return appropriate action based on decision
        match decision {
            ApprovalDecision::Approved | ApprovalDecision::ApprovedSession => HookAction::Continue,
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

    fn create_test_policy() -> ApprovalPolicy {
        ApprovalPolicy {
            require_approval: vec!["shell".to_string()],
            timeout_secs: 60,
            auto_approve_autonomous: false,
            auto_approve: false,
        }
    }

    #[tokio::test]
    async fn test_approval_hook_skips_non_before_tool_call_events() {
        let policy = create_test_policy();
        let manager = Arc::new(ApprovalManager::new(policy.clone()));
        let allow_list = Arc::new(RwLock::new(RuntimeAllowList::new()));
        let hook = ApprovalHook::new(manager, policy, allow_list, "test-agent");

        let ctx = ToolHookContext {
            event: HookEvent::AfterToolCall,
            tool_name: "shell".to_string(),
            tool_call_id: "test-id".to_string(),
            tool_input: serde_json::json!({"command": "echo test"}),
            tool_result: None,
            error: None,
            tool_manager: None,
            thread_event_sender: None,
            thread_id: None,
            turn_number: None,
        };

        let action = hook.on_tool_event(&ctx).await;
        assert!(matches!(action, HookAction::Continue));
    }

    #[tokio::test]
    async fn test_approval_hook_skips_tools_not_in_policy() {
        let policy = create_test_policy();
        let manager = Arc::new(ApprovalManager::new(policy.clone()));
        let allow_list = Arc::new(RwLock::new(RuntimeAllowList::new()));
        let hook = ApprovalHook::new(manager, policy, allow_list, "test-agent");

        let ctx = ToolHookContext {
            event: HookEvent::BeforeToolCall,
            tool_name: "read_file".to_string(),
            tool_call_id: "test-id".to_string(),
            tool_input: serde_json::json!({"path": "/tmp/test.txt"}),
            tool_result: None,
            error: None,
            tool_manager: None,
            thread_event_sender: None,
            thread_id: None,
            turn_number: None,
        };

        let action = hook.on_tool_event(&ctx).await;
        assert!(matches!(action, HookAction::Continue));
    }

    #[tokio::test]
    async fn test_approval_hook_skips_tools_in_allow_list() {
        let policy = create_test_policy();
        let manager = Arc::new(ApprovalManager::new(policy.clone()));
        let allow_list = Arc::new(RwLock::new(RuntimeAllowList::new()));

        // Add "shell" to the allow list
        allow_list.write().unwrap().allow_tool("shell");

        let hook = ApprovalHook::new(manager, policy, allow_list, "test-agent");

        let ctx = ToolHookContext {
            event: HookEvent::BeforeToolCall,
            tool_name: "shell".to_string(),
            tool_call_id: "test-id".to_string(),
            tool_input: serde_json::json!({"command": "echo test"}),
            tool_result: None,
            error: None,
            tool_manager: None,
            thread_event_sender: None,
            thread_id: None,
            turn_number: None,
        };

        let action = hook.on_tool_event(&ctx).await;
        assert!(matches!(action, HookAction::Continue));
    }

    #[tokio::test]
    async fn test_approval_hook_skips_when_allow_all_set() {
        let policy = create_test_policy();
        let manager = Arc::new(ApprovalManager::new(policy.clone()));
        let allow_list = Arc::new(RwLock::new(RuntimeAllowList::new()));

        // Set allow_all
        allow_list.write().unwrap().allow_all();

        let hook = ApprovalHook::new(manager, policy, allow_list, "test-agent");

        let ctx = ToolHookContext {
            event: HookEvent::BeforeToolCall,
            tool_name: "shell".to_string(),
            tool_call_id: "test-id".to_string(),
            tool_input: serde_json::json!({"command": "rm -rf /"}),
            tool_result: None,
            error: None,
            tool_manager: None,
            thread_event_sender: None,
            thread_id: None,
            turn_number: None,
        };

        let action = hook.on_tool_event(&ctx).await;
        assert!(matches!(action, HookAction::Continue));
    }
}
