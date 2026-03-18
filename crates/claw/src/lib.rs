// Allow unused code in internal modules during transition
#![allow(dead_code)]
#![allow(clippy::too_many_arguments)]

// === 内部模块 (不对外暴露) ===
pub mod agents; // Public for argus-thread
pub(crate) mod db;
pub(crate) mod job;
pub(crate) mod scheduler;
pub(crate) mod workflow;

// === 公开模块 ===
pub mod claw; // AppContext
pub mod error; // AgentError
pub mod protocol; // 稳定 DTO
pub mod user; // User management

// Approval types re-exported from argus-approval
pub use argus_approval::{
    ApprovalDecision, ApprovalError, ApprovalEvent, ApprovalHook, ApprovalManager,
    ApprovalPolicy, ApprovalRequest, ApprovalResponse, RuntimeAllowList,
};

// === 稳定公共 API 重导出 ===

// 核心入口
pub use claw::AppContext;
pub use error::AgentError;

// Agent API (稳定对话接口)
pub use agents::{AgentBuilder, AgentId, ThreadConfig};
pub use agents::{AgentRecord, AgentRuntimeInfo, ThreadInfo};

// Protocol Types (稳定 DTO)
pub use protocol::{
    LlmEventStream, LlmStreamEvent, RiskLevel, RuntimeAgentHandle, ThreadEvent, ThreadId,
    ThreadMessageSnapshot, ThreadSnapshot, TokenUsage, ToolCallSnapshot,
};

// LLM Provider Types (DTO)
pub use db::DbError;
pub use db::llm::{
    LlmProviderId, LlmProviderKind, LlmProviderRecord, ProviderSecretStatus, ProviderTestResult,
    ProviderTestStatus, SecretString,
};

// Tool Types - from argus-tool
pub use argus_tool::{GlobTool, GrepTool, NamedTool, ReadTool, ShellTool, ToolError, ToolManager};

// LLM Types - from argus-protocol (types) and argus-llm (ProviderManager)
pub use argus_llm::ProviderManager;
pub use argus_protocol::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LlmError, LlmProvider, ProviderId, Role,
    ToolCall, ToolCompletionRequest, ToolCompletionResponse, ToolDefinition,
};

// User Types
pub use user::UserInfo;

// === Dev Feature 重导出 ===
#[cfg(feature = "dev")]
pub use agents::Agent;
#[cfg(feature = "dev")]
pub use agents::AgentRepository;
#[cfg(feature = "dev")]
pub use agents::turn;
#[cfg(feature = "dev")]
pub use argus_approval::{ApprovalManager, ApprovalPolicy, RuntimeAllowList};
#[cfg(feature = "dev")]
pub use argus_repository::{
    JobRecord, JobRepository, JobType, MessageRecord, ThreadRecord, ThreadRepository,
    WorkflowRecord, WorkflowRepository, WorkflowStatus,
};
#[cfg(feature = "dev")]
pub use db::llm::LlmProviderRepository;
#[cfg(feature = "dev")]
pub use workflow::{JobId, WorkflowId};
