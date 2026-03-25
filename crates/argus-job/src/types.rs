//! Job-related types for dispatch and results.

use serde::{Deserialize, Serialize};

use argus_protocol::AgentId;

/// Arguments for dispatching a job.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobDispatchArgs {
    /// The prompt/task description for the job.
    pub prompt: String,
    /// The agent ID to use for this job.
    pub agent_id: AgentId,
    /// Optional context JSON for the job.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub context: Option<serde_json::Value>,
}

/// Result of a completed job (serialized into tool responses).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobResult {
    /// Whether the job succeeded.
    pub success: bool,
    /// Output or error message.
    pub message: String,
    /// Token usage if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<argus_protocol::TokenUsage>,
}
