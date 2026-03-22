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
    /// Whether to wait for the result synchronously.
    #[serde(default)]
    pub wait_for_result: bool,
}

/// Result of a job dispatch.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobDispatchResult {
    /// The job ID.
    pub job_id: String,
    /// Status: "submitted" or "completed".
    pub status: String,
    /// Result data if wait_for_result was true.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<JobResult>,
}

/// Result of a completed job.
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

/// Job status for SSE events.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobStatusEvent {
    /// The job ID.
    pub job_id: String,
    /// Status: "completed", "failed", "stuck".
    pub status: String,
    /// Optional session ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    /// Result message.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}
