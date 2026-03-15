// Allow unused code in internal modules during transition
#![allow(dead_code)]
#![allow(clippy::too_many_arguments)]

// === 内部模块 (不对外暴露) ===
pub(crate) mod agents;
pub(crate) mod api;
pub(crate) mod db;
pub(crate) mod job;
pub(crate) mod llm;
pub(crate) mod scheduler;
pub(crate) mod tool;
pub(crate) mod workflow;

// === 公开模块 ===
pub mod claw; // AppContext
pub mod error; // AgentError
pub mod protocol; // 稳定 DTO

// approval: dev feature 下公开，否则 crate 内部
#[cfg(feature = "dev")]
pub mod approval;
#[cfg(not(feature = "dev"))]
pub(crate) mod approval;

// === 稳定公共 API 重导出 ===

// 核心入口
pub use claw::AppContext;
pub use error::AgentError;

// Agent API (稳定对话接口)
pub use agents::{Agent, AgentBuilder, AgentId, ThreadConfig};

// Protocol Types (稳定 DTO)
pub use protocol::{
    ApprovalDecision, ApprovalRequest, ApprovalResponse, LlmStreamEvent, RiskLevel, ThreadEvent,
    ThreadId, TokenUsage,
};

// LLM Provider Types (DTO)
pub use db::DbError;
pub use db::llm::{
    LlmProviderId, LlmProviderKind, LlmProviderRecord, LlmProviderSummary, SecretString,
};

// Tool Types
pub use tool::{GlobTool, GrepTool, NamedTool, ReadTool, ShellTool, ToolError, ToolManager};

// === Dev Feature 重导出 ===
#[cfg(feature = "dev")]
pub use agents::turn;
#[cfg(feature = "dev")]
pub use agents::{AgentRecord, AgentRepository};
#[cfg(feature = "dev")]
pub use approval::{ApprovalManager, ApprovalPolicy};
#[cfg(feature = "dev")]
pub use db::llm::LlmProviderRepository;
#[cfg(feature = "dev")]
pub use db::sqlite::{
    self, SqliteAgentRepository, SqliteLlmProviderRepository, SqliteThreadRepository, connect,
    migrate,
};
#[cfg(feature = "dev")]
pub use db::thread::{MessageRecord, ThreadRecord, ThreadRepository};
#[cfg(feature = "dev")]
pub use db::{ApprovalRepository, SqliteJobRepository, SqliteWorkflowRepository};
#[cfg(feature = "dev")]
pub use job::{JobRecord, JobRepository, JobType};
#[cfg(feature = "dev")]
pub use llm::{ChatMessage, LLMManager, LlmEventStream, LlmProvider, Role};
#[cfg(feature = "dev")]
pub use workflow::{JobId, WorkflowId, WorkflowRecord, WorkflowRepository, WorkflowStatus};
