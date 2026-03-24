//! Job persistence types.

use std::fmt;

use serde::{Deserialize, Serialize};

use super::{AgentId, WorkflowId, WorkflowStatus};
use argus_protocol::{ThreadId, TokenUsage};

/// Result of a completed job (persisted in database).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobResult {
    /// Whether the job succeeded.
    pub success: bool,
    /// Output or error message.
    pub message: String,
    /// Token usage if available.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token_usage: Option<TokenUsage>,
}

/// The kind of job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobType {
    /// Standalone job.
    Standalone,
    /// Job within a workflow.
    Workflow,
    /// Scheduled cron job.
    Cron,
}

impl JobType {
    /// Returns the string representation of this job type.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Standalone => "standalone",
            Self::Workflow => "workflow",
            Self::Cron => "cron",
        }
    }

    /// Parses a job type from a string.
    pub fn parse_str(s: &str) -> Result<Self, String> {
        match s {
            "standalone" => Ok(Self::Standalone),
            "workflow" => Ok(Self::Workflow),
            "cron" => Ok(Self::Cron),
            _ => Err(format!("invalid job type: {s}")),
        }
    }
}

impl fmt::Display for JobType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Full job record stored in database.
pub struct JobRecord {
    pub id: WorkflowId,
    pub job_type: JobType,
    pub name: String,
    pub status: WorkflowStatus,
    pub agent_id: AgentId,
    pub context: Option<String>,
    pub prompt: String,
    pub thread_id: Option<ThreadId>,
    pub group_id: Option<String>,
    pub depends_on: Vec<WorkflowId>,
    pub cron_expr: Option<String>,
    pub scheduled_at: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
    /// Parent job ID (for subagent-dispatched jobs).
    pub parent_job_id: Option<WorkflowId>,
    /// Job execution result (set when job completes).
    pub result: Option<JobResult>,
}

#[cfg(test)]
impl JobRecord {
    /// Creates a minimal job record for testing.
    #[must_use]
    pub fn for_test(id: &str, agent_id: i64, name: &str, prompt: &str) -> Self {
        Self {
            id: WorkflowId::new(id),
            job_type: JobType::Standalone,
            name: name.to_string(),
            status: WorkflowStatus::Pending,
            agent_id: AgentId::new(agent_id),
            context: None,
            prompt: prompt.to_string(),
            thread_id: None,
            group_id: None,
            depends_on: vec![],
            cron_expr: None,
            scheduled_at: None,
            started_at: None,
            finished_at: None,
            parent_job_id: None,
            result: None,
        }
    }
}
