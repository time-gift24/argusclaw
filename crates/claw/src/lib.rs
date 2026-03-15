pub mod agents;
pub mod api;
pub mod claw;
pub mod db;
pub mod error;
pub mod job;
pub mod llm;
pub mod protocol;
pub mod scheduler;
pub mod tool;
pub mod workflow;

// Approval module: pub for dev feature (dev CLI commands), otherwise crate-internal
#[cfg(feature = "dev")]
pub mod approval;
#[cfg(not(feature = "dev"))]
pub(crate) mod approval;

pub use claw::AppContext;
pub use error::AgentError;
pub use protocol::{
    ApprovalDecision, ApprovalRequest, ApprovalResponse, LlmStreamEvent, RiskLevel, ThreadEvent,
    ThreadId, TokenUsage,
};
pub use tool::{NamedTool, ToolError, ToolManager};
