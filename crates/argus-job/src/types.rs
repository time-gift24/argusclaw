//! Job-related types for dispatch and results.

use serde::{Deserialize, Serialize};

use argus_protocol::{AgentId, ThreadId};

use crate::workflow_manager::{AppendWorkflowNode, WorkflowExecutionProgress};

/// Arguments accepted by the unified `scheduler` tool.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum SchedulerToolArgs {
    /// List subagents available to the current agent.
    ListSubagents,
    /// Dispatch a background job to a subagent.
    DispatchJob {
        /// The prompt/task description for the job.
        prompt: String,
        /// The agent ID to use for this job.
        agent_id: AgentId,
        /// Optional context JSON for the job.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        context: Option<serde_json::Value>,
    },
    /// Proactively check a dispatched job result.
    GetJobResult {
        /// The job ID returned by `scheduler` with `action=dispatch_job`.
        job_id: String,
        /// Whether to consume the completed result and prevent future auto-replay.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        consume: Option<bool>,
    },
    /// Start a workflow execution.
    StartWorkflow {
        /// The workflow template ID to instantiate.
        template_id: String,
        /// Optional template version. When omitted, the latest version is used.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        template_version: Option<i64>,
        /// Append-only workflow nodes supplied by the main agent.
        #[serde(default)]
        extra_nodes: Vec<AppendWorkflowNode>,
    },
    /// Query workflow progress.
    GetWorkflowProgress {
        /// The workflow execution ID returned by `scheduler` with `action=start_workflow`.
        workflow_execution_id: String,
    },
}

/// Result of `scheduler` with `action=start_workflow`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StartWorkflowResult {
    /// The instantiated workflow execution ID.
    pub workflow_execution_id: String,
    /// Immediate progress snapshot after instantiation.
    pub progress: WorkflowExecutionProgress,
}

/// Result of `scheduler` with `action=get_workflow_progress`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetWorkflowProgressResult {
    /// Latest workflow progress snapshot.
    pub progress: WorkflowExecutionProgress,
}

/// Result of a completed job (serialized into `scheduler` responses).
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

/// Summary of a subagent returned by `scheduler` with `action=list_subagents`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubagentSummary {
    /// Agent ID.
    pub agent_id: AgentId,
    /// Human-readable subagent name.
    pub display_name: String,
    /// Subagent description.
    pub description: String,
}

/// In-memory request model used by ThreadPool orchestration.
#[derive(Debug, Clone)]
pub struct ThreadPoolJobRequest {
    /// Source thread where `scheduler` was invoked with `action=dispatch_job`.
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
