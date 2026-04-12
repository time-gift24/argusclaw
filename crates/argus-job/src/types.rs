//! Job-related in-memory orchestration types.

use argus_protocol::{AgentId, ThreadId};

/// In-memory request model used by job-runtime orchestration.
#[derive(Debug, Clone)]
pub struct JobRuntimeRequest {
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
