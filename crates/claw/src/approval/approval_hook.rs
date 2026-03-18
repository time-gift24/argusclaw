//! Approval hook - integrates with Turn execution through the hook system
//!
//! This hook:
//! - Checks if a tool requires approval based on the current policy
//! - If so, creates an approval request and waits for approval/denied/timeout
//! - Returns `HookAction::Continue` or `Block(reason)` based on the decision
//! - Sends ThreadEvents (WaitingForApproval, ApprovalResolved) to notify frontend
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
use chrono::Utc;
use std::sync::Arc;

use crate::approval::{ApprovalManager, ApprovalPolicy};
use crate::protocol::{
    ApprovalDecision, ApprovalRequest, ApprovalResponse, HookAction, HookEvent, HookHandler,
    RiskLevel, ThreadEvent, ToolHookContext,
};

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
            .map(|tm| tm.risk_level())
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
        let hook = ApprovalHook::new(manager, policy, "test-agent");

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

    // ========================================================================
    // Integration tests (migrated from tests/approval_integration_test.rs)
    // ========================================================================

    use std::sync::Mutex;
    use std::time::Duration;

    use rust_decimal::Decimal;

    use crate::agents::turn::{TurnConfig, TurnInputBuilder, execute_turn};
    use argus_protocol::llm::{
        ChatMessage, FinishReason, LlmError, LlmProvider, ToolCall, ToolCompletionRequest,
        ToolCompletionResponse, ToolDefinition,
    };
    use argus_tool::{NamedTool, ToolError, ToolManager};

    struct SequentialMockProvider {
        responses: Mutex<Vec<ToolCompletionResponse>>,
        call_count: Mutex<usize>,
    }

    impl SequentialMockProvider {
        fn new(responses: Vec<ToolCompletionResponse>) -> Self {
            Self {
                responses: Mutex::new(responses),
                call_count: Mutex::new(0),
            }
        }
    }

    #[async_trait]
    impl LlmProvider for SequentialMockProvider {
        fn model_name(&self) -> &str {
            "mock-approval-test"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: argus_protocol::llm::CompletionRequest,
        ) -> Result<argus_protocol::llm::CompletionResponse, LlmError> {
            unimplemented!("complete not used in turn execution")
        }

        async fn complete_with_tools(
            &self,
            _request: ToolCompletionRequest,
        ) -> Result<ToolCompletionResponse, LlmError> {
            let mut count = self.call_count.lock().unwrap();
            let responses = self.responses.lock().unwrap();
            let response =
                responses
                    .get(*count)
                    .cloned()
                    .ok_or_else(|| LlmError::RequestFailed {
                        provider: "mock".to_string(),
                        reason: format!("No response for call {}", count),
                    })?;
            *count += 1;
            Ok(response)
        }
    }

    struct DangerousTool;

    #[async_trait]
    impl NamedTool for DangerousTool {
        fn name(&self) -> &str {
            "dangerous_tool"
        }

        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "dangerous_tool".to_string(),
                description: "A dangerous tool that requires approval".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "action": { "type": "string" }
                    },
                    "required": ["action"]
                }),
            }
        }

        fn risk_level(&self) -> RiskLevel {
            RiskLevel::Critical
        }

        async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value, ToolError> {
            let action = args
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            Ok(serde_json::json!({ "result": format!("Executed: {}", action) }))
        }
    }

    struct SafeTool;

    #[async_trait]
    impl NamedTool for SafeTool {
        fn name(&self) -> &str {
            "safe_tool"
        }

        fn definition(&self) -> ToolDefinition {
            ToolDefinition {
                name: "safe_tool".to_string(),
                description: "A safe tool that doesn't require approval".to_string(),
                parameters: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "message": { "type": "string" }
                    }
                }),
            }
        }

        fn risk_level(&self) -> RiskLevel {
            RiskLevel::Low
        }

        async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value, ToolError> {
            let message = args
                .get("message")
                .and_then(|v| v.as_str())
                .unwrap_or("hello");
            Ok(serde_json::json!({ "echo": message }))
        }
    }

    fn create_approval_responses_blocked() -> Vec<ToolCompletionResponse> {
        vec![
            ToolCompletionResponse {
                content: Some("I'll use the dangerous tool.".to_string()),
                reasoning_content: None,
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "dangerous_tool".to_string(),
                    arguments: serde_json::json!({"action": "delete everything"}),
                }],
                finish_reason: FinishReason::ToolUse,
                input_tokens: 100,
                output_tokens: 50,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
            ToolCompletionResponse {
                content: Some("I understand the tool was blocked.".to_string()),
                reasoning_content: None,
                tool_calls: vec![],
                finish_reason: FinishReason::Stop,
                input_tokens: 50,
                output_tokens: 20,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
        ]
    }

    fn create_approval_responses_allowed() -> Vec<ToolCompletionResponse> {
        vec![
            ToolCompletionResponse {
                content: Some("I'll use the dangerous tool.".to_string()),
                reasoning_content: None,
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "dangerous_tool".to_string(),
                    arguments: serde_json::json!({"action": "read config"}),
                }],
                finish_reason: FinishReason::ToolUse,
                input_tokens: 100,
                output_tokens: 50,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
            ToolCompletionResponse {
                content: Some("Done!".to_string()),
                reasoning_content: None,
                tool_calls: vec![],
                finish_reason: FinishReason::Stop,
                input_tokens: 50,
                output_tokens: 20,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
        ]
    }

    #[tokio::test]
    async fn test_approval_hook_blocks_dangerous_tool() {
        let policy = ApprovalPolicy {
            require_approval: vec!["dangerous_tool".to_string()],
            timeout_secs: 60,
            auto_approve: false,
            auto_approve_autonomous: false,
        };

        let approval_manager = Arc::new(ApprovalManager::new(policy.clone()));

        let hooks = Arc::new(crate::protocol::HookRegistry::new());
        let approval_hook = ApprovalHook::new(Arc::clone(&approval_manager), policy, "test-agent");
        hooks.register(HookEvent::BeforeToolCall, Arc::new(approval_hook));

        let tool_manager = Arc::new(ToolManager::new());
        tool_manager.register(Arc::new(DangerousTool));

        let provider = Arc::new(SequentialMockProvider::new(
            create_approval_responses_blocked(),
        ));

        let input = TurnInputBuilder::new()
            .provider(provider)
            .messages(vec![ChatMessage::user("Use the dangerous tool")])
            .tool_manager(tool_manager)
            .tool_ids(vec!["dangerous_tool".to_string()])
            .hooks(hooks)
            .build()
            .unwrap();

        let manager_clone = Arc::clone(&approval_manager);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let pending = manager_clone.list_pending();
            if !pending.is_empty() {
                let _ = manager_clone.resolve(
                    pending[0].id,
                    ApprovalDecision::Denied,
                    Some("test-denied".to_string()),
                );
            }
        });

        let config = TurnConfig::default();
        let result = execute_turn(input, config).await;

        assert!(
            result.is_ok(),
            "Turn should complete even with denied approval"
        );
        let output = result.unwrap();

        let has_blocked_message = output
            .messages
            .iter()
            .any(|m| m.content.contains("blocked") || m.content.contains("denied"));
        assert!(
            has_blocked_message,
            "Tool should have been blocked: {:?}",
            output.messages
        );
    }

    #[tokio::test]
    async fn test_approval_hook_allows_approved_tool() {
        let policy = ApprovalPolicy {
            require_approval: vec!["dangerous_tool".to_string()],
            timeout_secs: 60,
            auto_approve: false,
            auto_approve_autonomous: false,
        };

        let approval_manager = Arc::new(ApprovalManager::new(policy.clone()));

        let hooks = Arc::new(crate::protocol::HookRegistry::new());
        let approval_hook = ApprovalHook::new(Arc::clone(&approval_manager), policy, "test-agent");
        hooks.register(HookEvent::BeforeToolCall, Arc::new(approval_hook));

        let tool_manager = Arc::new(ToolManager::new());
        tool_manager.register(Arc::new(DangerousTool));

        let provider = Arc::new(SequentialMockProvider::new(
            create_approval_responses_allowed(),
        ));

        let input = TurnInputBuilder::new()
            .provider(provider)
            .messages(vec![ChatMessage::user("Use the dangerous tool")])
            .tool_manager(tool_manager)
            .tool_ids(vec!["dangerous_tool".to_string()])
            .hooks(hooks)
            .build()
            .unwrap();

        let manager_clone = Arc::clone(&approval_manager);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let pending = manager_clone.list_pending();
            if !pending.is_empty() {
                let _ = manager_clone.resolve(
                    pending[0].id,
                    ApprovalDecision::Approved,
                    Some("test-approver".to_string()),
                );
            }
        });

        let config = TurnConfig::default();
        let result = execute_turn(input, config).await;

        assert!(result.is_ok(), "Turn should complete with approved tool");
        let output = result.unwrap();

        let has_executed = output
            .messages
            .iter()
            .any(|m| m.content.contains("Executed") || m.content.contains("read config"));
        assert!(
            has_executed,
            "Tool should have been executed: {:?}",
            output.messages
        );
    }

    #[tokio::test]
    async fn test_safe_tool_bypasses_approval() {
        let policy = ApprovalPolicy {
            require_approval: vec!["dangerous_tool".to_string()],
            timeout_secs: 60,
            auto_approve: false,
            auto_approve_autonomous: false,
        };

        let approval_manager = Arc::new(ApprovalManager::new(policy.clone()));

        let hooks = Arc::new(crate::protocol::HookRegistry::new());
        let approval_hook = ApprovalHook::new(Arc::clone(&approval_manager), policy, "test-agent");
        hooks.register(HookEvent::BeforeToolCall, Arc::new(approval_hook));

        let tool_manager = Arc::new(ToolManager::new());
        tool_manager.register(Arc::new(SafeTool));

        let responses = vec![
            ToolCompletionResponse {
                content: Some("I'll use the safe tool.".to_string()),
                reasoning_content: None,
                tool_calls: vec![ToolCall {
                    id: "call-1".to_string(),
                    name: "safe_tool".to_string(),
                    arguments: serde_json::json!({"message": "hello world"}),
                }],
                finish_reason: FinishReason::ToolUse,
                input_tokens: 100,
                output_tokens: 50,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
            ToolCompletionResponse {
                content: Some("Done!".to_string()),
                reasoning_content: None,
                tool_calls: vec![],
                finish_reason: FinishReason::Stop,
                input_tokens: 50,
                output_tokens: 20,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            },
        ];
        let provider = Arc::new(SequentialMockProvider::new(responses));

        let input = TurnInputBuilder::new()
            .provider(provider)
            .messages(vec![ChatMessage::user("Use the safe tool")])
            .tool_manager(tool_manager)
            .tool_ids(vec!["safe_tool".to_string()])
            .hooks(hooks)
            .build()
            .unwrap();

        let config = TurnConfig::default();
        let result = execute_turn(input, config).await;

        assert!(
            result.is_ok(),
            "Turn should complete without approval for safe tool"
        );
        let output = result.unwrap();

        assert!(
            approval_manager.list_pending().is_empty(),
            "Should have no pending approvals"
        );

        let has_echo = output
            .messages
            .iter()
            .any(|m| m.content.contains("echo") || m.content.contains("hello world"));
        assert!(
            has_echo,
            "Safe tool should have been executed: {:?}",
            output.messages
        );
    }

    #[tokio::test]
    async fn test_approval_events_broadcast_to_thread() {
        use tokio::sync::broadcast;

        let policy = ApprovalPolicy {
            require_approval: vec!["dangerous_tool".to_string()],
            timeout_secs: 60,
            auto_approve: false,
            auto_approve_autonomous: false,
        };

        let approval_manager = Arc::new(ApprovalManager::new(policy.clone()));

        let (thread_event_tx, mut thread_event_rx) = broadcast::channel::<ThreadEvent>(16);

        let hooks = Arc::new(crate::protocol::HookRegistry::new());
        let approval_hook = ApprovalHook::new(Arc::clone(&approval_manager), policy, "test-agent");
        hooks.register(HookEvent::BeforeToolCall, Arc::new(approval_hook));

        let tool_manager = Arc::new(ToolManager::new());
        tool_manager.register(Arc::new(DangerousTool));

        let provider = Arc::new(SequentialMockProvider::new(
            create_approval_responses_allowed(),
        ));

        let thread_id = crate::protocol::ThreadId::new();
        let input = TurnInputBuilder::new()
            .provider(provider)
            .messages(vec![ChatMessage::user("Use the dangerous tool")])
            .tool_manager(tool_manager)
            .tool_ids(vec!["dangerous_tool".to_string()])
            .hooks(hooks)
            .thread_event_sender(thread_event_tx)
            .thread_id(thread_id.inner().to_string())
            .build()
            .unwrap();

        let manager_clone = Arc::clone(&approval_manager);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(100)).await;
            let pending = manager_clone.list_pending();
            if !pending.is_empty() {
                let _ = manager_clone.resolve(
                    pending[0].id,
                    ApprovalDecision::Approved,
                    Some("test-approver".to_string()),
                );
            }
        });

        let config = TurnConfig::default();
        let result = execute_turn(input, config).await;

        assert!(result.is_ok(), "Turn should complete with approved tool");

        let event = thread_event_rx.recv().await;
        assert!(
            matches!(event, Ok(ThreadEvent::WaitingForApproval { .. })),
            "Expected WaitingForApproval event, got: {:?}",
            event
        );

        let event = thread_event_rx.recv().await;
        assert!(
            matches!(event, Ok(ThreadEvent::ApprovalResolved { .. })),
            "Expected ApprovalResolved event, got: {:?}",
            event
        );
    }
}
