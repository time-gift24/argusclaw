//! Job-related types for dispatch and results.

use serde::{Deserialize, Serialize};

use argus_protocol::{AgentId, ThreadId};

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

/// Arguments for proactively checking a dispatched job result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetJobResultArgs {
    /// The job ID returned by `dispatch_job`.
    pub job_id: String,
    /// Whether to consume the completed result and prevent future auto-replay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consume: Option<bool>,
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
    /// Agent ID that handled the subagent work.
    pub agent_id: AgentId,
    /// Human-readable subagent name.
    pub agent_display_name: String,
    /// Subagent description.
    pub agent_description: String,
}

/// In-memory request model used by ThreadPool orchestration.
#[derive(Debug, Clone)]
pub struct ThreadPoolJobRequest {
    /// Source thread where dispatch_job was invoked.
    pub originating_thread_id: ThreadId,
    /// Stable job ID for lookup and result correlation.
    pub job_id: String,
    /// Agent selected to execute the background task.
    pub agent_id: AgentId,
    /// Prompt that drives the subagent execution.
    pub prompt: String,
    /// Optional context payload.
    pub context: Option<serde_json::Value>,
}
