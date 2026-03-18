pub mod ids;
pub mod error;
pub mod config;
pub mod events;
pub mod approval;
pub mod hooks;
pub mod risk_level;
pub mod token_usage;
pub mod llm;
pub mod tool;

pub use ids::{SessionId, ThreadId, AgentId, ProviderId};
pub use error::{ArgusError, Result};
pub use approval::{ApprovalDecision, ApprovalEvent, ApprovalRequest, ApprovalResponse};
pub use risk_level::RiskLevel;
pub use token_usage::TokenUsage;
pub use hooks::{
    BeforeCallLLMContext, BeforeCallLLMResult, HookAction, HookEvent, HookHandler, HookRegistry,
    ToolHookContext,
};
pub use events::ThreadEvent;

pub use llm::{
    ChatMessage, CompletionRequest, CompletionResponse, ContentPart, FinishReason,
    ImageUrl, LlmError, LlmEventStream, LlmProvider, LlmStreamEvent, ModelMetadata,
    ProviderCapabilities, Role, sanitize_tool_messages, ThinkingConfig, ThinkingMode,
    ToolCall, ToolCallDelta, ToolCompletionRequest, ToolCompletionResponse, ToolDefinition,
    ToolResult,
    // Provider management types
    LlmProviderId, LlmProviderKind, LlmProviderKindParseError, LlmProviderRecord,
    LlmProviderSummary, ProviderSecretStatus, ProviderTestResult, ProviderTestStatus, SecretString,
    LlmProviderRepository,
};

pub use tool::{NamedTool, ToolError};
