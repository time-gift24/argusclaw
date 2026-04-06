pub mod account;
pub mod agent;
pub mod config;
pub mod error;
pub mod events;
pub mod hooks;
pub mod http_client;
pub mod ids;
pub mod llm;
pub mod mcp;
pub mod message_override;
pub mod plan;
pub mod risk_level;
pub mod safety;
pub mod ssrf;
pub mod token_usage;
pub mod tool;

pub use account::{AccountCredentials, AccountRepository};
pub use agent::{AgentRecord, AgentType};
pub use error::{ArgusError, Result};
pub use events::{
    MailboxMessage, MailboxMessageType, QueuedUserMessage, ThreadCommand, ThreadControlEvent,
    ThreadEvent, ThreadJobResult, ThreadMailbox, ThreadPoolEventReason, ThreadPoolRuntimeKind,
    ThreadPoolRuntimeRef, ThreadPoolRuntimeSummary, ThreadPoolSnapshot, ThreadPoolState,
    ThreadRuntimeSnapshot, ThreadRuntimeStatus,
};
pub use hooks::{HookAction, HookEvent, HookHandler, HookRegistry, ToolHookContext};
pub use ids::{AgentId, ProviderId, SessionId, ThreadId};
pub use mcp::{
    AgentMcpBinding, AgentMcpServerBinding, AgentMcpToolBinding, McpDiscoveredToolRecord,
    McpServerRecord, McpServerStatus, McpToolResolver, McpTransportConfig, McpTransportKind,
    McpUnavailableServerSummary, ResolvedMcpTools, ThreadNoticeLevel,
};
pub use message_override::MessageOverride;
pub use plan::{PlanItemArg, StepStatus, UpdatePlanArgs};
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
    LlmProviderRecordJson,
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
    ToolDefinition,
    ToolResult,
    sanitize_tool_messages,
};

pub use ssrf::{
    MAX_RESPONSE_SIZE, MAX_TIMEOUT_SECS, is_blocked_ip, is_blocked_ip_v4, is_blocked_ip_v6,
    validate_url,
};
pub use tool::{NamedTool, ToolError, ToolExecutionContext};

pub mod provider_resolver;
pub use provider_resolver::ProviderResolver;

pub use safety::{OutputWarning, SafetyConfig, sanitize_tool_output};

#[cfg(test)]
#[test]
fn thread_pool_snapshot_round_trips_through_json() {
    events::assert_thread_pool_snapshot_round_trip();
}
