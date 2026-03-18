pub mod approval;
pub mod config;
pub mod error;
pub mod events;
pub mod hooks;
pub mod ids;
pub mod llm;
pub mod risk_level;
pub mod token_usage;
pub mod tool;

pub use approval::{ApprovalDecision, ApprovalEvent, ApprovalRequest, ApprovalResponse};
pub use error::{ArgusError, Result};
pub use events::ThreadEvent;
pub use hooks::{
    BeforeCallLLMContext, BeforeCallLLMResult, HookAction, HookContext, HookEvent, HookHandler,
    HookRegistry, ToolHookContext,
};
pub use ids::{AgentId, ProviderId, SessionId, ThreadId};
pub use risk_level::RiskLevel;
pub use token_usage::TokenUsage;

pub use llm::{
    ChatMessage,
    CompletionRequest,
    CompletionResponse,
    ContentPart,
    FinishReason,
    ImageUrl,
    LlmError,
    LlmEventStream,
    LlmProvider,
    // Provider management types
    LlmProviderId,
    LlmProviderKind,
    LlmProviderKindParseError,
    LlmProviderRecord,
    LlmProviderRepository,
    LlmStreamEvent,
    ModelMetadata,
    ProviderCapabilities,
    ProviderSecretStatus,
    ProviderTestResult,
    ProviderTestStatus,
    Role,
    SecretString,
    ThinkingConfig,
    ThinkingMode,
    ToolCall,
    ToolCallDelta,
    ToolCompletionRequest,
    ToolCompletionResponse,
    ToolDefinition,
    ToolResult,
    sanitize_tool_messages,
};

pub use tool::{NamedTool, ToolError};
