//! Integration test for Approval flow with ShellTool.
//!
//! This test verifies the complete approval flow:
//! 1. ShellTool is registered with Critical risk level
//! 2. ApprovalHook is registered to HookRegistry
//! 3. When shell tool is called, ApprovalHook intercepts
//! 4. Approval is requested and must be resolved
//! 5. Tool execution proceeds or is blocked based on decision

use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use rust_decimal::Decimal;

use claw::agents::turn::{HookEvent, HookRegistry, TurnConfig, TurnInputBuilder, execute_turn};
use claw::approval::{ApprovalDecision, ApprovalHook, ApprovalManager, ApprovalPolicy};
use claw::llm::{
    ChatMessage, FinishReason, LlmError, LlmProvider, ToolCall, ToolCompletionRequest,
    ToolCompletionResponse, ToolDefinition,
};
use claw::protocol::RiskLevel;
use claw::tool::{NamedTool, ToolError, ToolManager};

// ============================================================================
// Mock Provider
// ============================================================================

/// Mock LLM provider that returns pre-defined responses in sequence.
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
        _request: claw::llm::CompletionRequest,
    ) -> Result<claw::llm::CompletionResponse, LlmError> {
        unimplemented!("complete not used in turn execution")
    }

    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> Result<ToolCompletionResponse, LlmError> {
        let mut count = self.call_count.lock().unwrap();
        let responses = self.responses.lock().unwrap();
        let response = responses
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

// ============================================================================
// Test Tools
// ============================================================================

/// A tool that requires approval (Critical risk).
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

/// A safe tool that doesn't require approval.
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

// ============================================================================
// Tests
// ============================================================================

#[tokio::test]
async fn test_approval_hook_blocks_dangerous_tool() {
    // Create policy that requires approval for dangerous_tool
    let policy = ApprovalPolicy {
        require_approval: vec!["dangerous_tool".to_string()],
        timeout_secs: 60,
        auto_approve: false,
        auto_approve_autonomous: false,
    };

    let approval_manager = Arc::new(ApprovalManager::new(policy.clone()));

    // Create hook registry and register approval hook
    let hooks = Arc::new(HookRegistry::new());
    let approval_hook = ApprovalHook::new(Arc::clone(&approval_manager), policy, "test-agent");
    hooks.register(HookEvent::BeforeToolCall, Arc::new(approval_hook));

    // Create tool manager and register dangerous tool
    let tool_manager = Arc::new(ToolManager::new());
    tool_manager.register(Arc::new(DangerousTool));

    // Create provider that will call dangerous tool
    let responses = vec![
        // First call: request dangerous tool
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
        // Second call: respond to tool result (blocked message)
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
    ];
    let provider = Arc::new(SequentialMockProvider::new(responses));

    // Create turn input
    let input = TurnInputBuilder::new()
        .provider(provider)
        .messages(vec![ChatMessage::user("Use the dangerous tool")])
        .tool_manager(tool_manager)
        .tool_ids(vec!["dangerous_tool".to_string()])
        .hooks(hooks)
        .build();

    // Spawn a task to deny the approval after a short delay
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

    // Execute turn
    let config = TurnConfig::default();
    let result = execute_turn(input, config).await;

    // Should succeed (tool result contains blocked message)
    assert!(
        result.is_ok(),
        "Turn should complete even with denied approval"
    );
    let output = result.unwrap();

    // Verify the tool was blocked
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
    // Create policy that requires approval for dangerous_tool
    let policy = ApprovalPolicy {
        require_approval: vec!["dangerous_tool".to_string()],
        timeout_secs: 60,
        auto_approve: false,
        auto_approve_autonomous: false,
    };

    let approval_manager = Arc::new(ApprovalManager::new(policy.clone()));

    // Create hook registry and register approval hook
    let hooks = Arc::new(HookRegistry::new());
    let approval_hook = ApprovalHook::new(Arc::clone(&approval_manager), policy, "test-agent");
    hooks.register(HookEvent::BeforeToolCall, Arc::new(approval_hook));

    // Create tool manager and register dangerous tool
    let tool_manager = Arc::new(ToolManager::new());
    tool_manager.register(Arc::new(DangerousTool));

    // Create provider that will call dangerous tool
    let responses = vec![
        // First call: request dangerous tool
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
        // Second call: respond to tool result
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

    // Create turn input
    let input = TurnInputBuilder::new()
        .provider(provider)
        .messages(vec![ChatMessage::user("Use the dangerous tool")])
        .tool_manager(tool_manager)
        .tool_ids(vec!["dangerous_tool".to_string()])
        .hooks(hooks)
        .build();

    // Spawn a task to approve after a short delay
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

    // Execute turn
    let config = TurnConfig::default();
    let result = execute_turn(input, config).await;

    // Should succeed
    assert!(result.is_ok(), "Turn should complete with approved tool");
    let output = result.unwrap();

    // Verify the tool was executed (not blocked)
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
    // Create policy that requires approval for dangerous_tool (not safe_tool)
    let policy = ApprovalPolicy {
        require_approval: vec!["dangerous_tool".to_string()],
        timeout_secs: 60,
        auto_approve: false,
        auto_approve_autonomous: false,
    };

    let approval_manager = Arc::new(ApprovalManager::new(policy.clone()));

    // Create hook registry and register approval hook
    let hooks = Arc::new(HookRegistry::new());
    let approval_hook = ApprovalHook::new(Arc::clone(&approval_manager), policy, "test-agent");
    hooks.register(HookEvent::BeforeToolCall, Arc::new(approval_hook));

    // Create tool manager and register safe tool
    let tool_manager = Arc::new(ToolManager::new());
    tool_manager.register(Arc::new(SafeTool));

    // Create provider that will call safe tool
    let responses = vec![
        // First call: request safe tool
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
        // Second call: respond to tool result
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

    // Create turn input
    let input = TurnInputBuilder::new()
        .provider(provider)
        .messages(vec![ChatMessage::user("Use the safe tool")])
        .tool_manager(tool_manager)
        .tool_ids(vec!["safe_tool".to_string()])
        .hooks(hooks)
        .build();

    // Execute turn - no need to approve since safe_tool is not in policy
    let config = TurnConfig::default();
    let result = execute_turn(input, config).await;

    // Should succeed immediately without needing approval
    assert!(
        result.is_ok(),
        "Turn should complete without approval for safe tool"
    );
    let output = result.unwrap();

    // Verify no pending approvals were created
    assert!(
        approval_manager.list_pending().is_empty(),
        "Should have no pending approvals"
    );

    // Verify the tool was executed
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
