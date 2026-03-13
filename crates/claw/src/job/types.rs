use std::fmt;

use serde::{Deserialize, Serialize};

use crate::agents::AgentId;
use crate::agents::thread::ThreadId;
use crate::workflow::{JobId, WorkflowStatus};

/// The kind of job: standalone one-off, part of a workflow, or recurring cron.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobType {
    Standalone,
    Workflow,
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
    ///
    /// # Errors
    /// Returns an error if the string is not a valid job type.
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

/// Where a job is stored and executed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobBackendKind {
    InMemory,
    Persistent,
}

impl JobBackendKind {
    /// Returns the string representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::InMemory => "in_memory",
            Self::Persistent => "persistent",
        }
    }

    /// Parses from string.
    pub fn parse_str(s: &str) -> Result<Self, String> {
        match s {
            "in_memory" => Ok(Self::InMemory),
            "persistent" => Ok(Self::Persistent),
            _ => Err(format!("invalid job backend kind: {s}")),
        }
    }
}

impl fmt::Display for JobBackendKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Execution status of a job (used by InMemoryJobBackend).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    Pending,
    Running,
    Succeeded,
    Failed,
    Cancelled,
    TimedOut,
}

impl JobStatus {
    /// Returns the string representation.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
        }
    }

    /// Returns true if this is a terminal state.
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::TimedOut
        )
    }
}

impl From<WorkflowStatus> for JobStatus {
    fn from(status: WorkflowStatus) -> Self {
        match status {
            WorkflowStatus::Pending => Self::Pending,
            WorkflowStatus::Running => Self::Running,
            WorkflowStatus::Succeeded => Self::Succeeded,
            WorkflowStatus::Failed => Self::Failed,
            WorkflowStatus::Cancelled => Self::Cancelled,
        }
    }
}

impl From<JobStatus> for WorkflowStatus {
    fn from(status: JobStatus) -> Self {
        match status {
            JobStatus::Pending => Self::Pending,
            JobStatus::Running => Self::Running,
            JobStatus::Succeeded => Self::Succeeded,
            JobStatus::Failed => Self::Failed,
            JobStatus::Cancelled => Self::Cancelled,
            JobStatus::TimedOut => Self::Failed, // TimedOut maps to Failed
        }
    }
}

/// Full job record stored in database.
pub struct JobRecord {
    pub id: JobId,
    pub job_type: JobType,
    pub name: String,
    pub status: WorkflowStatus,
    pub agent_id: AgentId,
    pub context: Option<String>,
    pub prompt: String,
    pub thread_id: Option<ThreadId>,
    pub group_id: Option<String>,
    pub depends_on: Vec<JobId>,
    pub cron_expr: Option<String>,
    pub scheduled_at: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}

#[cfg(test)]
impl JobRecord {
    /// Creates a minimal job record for testing.
    #[must_use]
    pub fn for_test(id: &str, agent_id: &str, name: &str, prompt: &str) -> Self {
        Self {
            id: JobId::new(id),
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
        }
    }
}

#[cfg(test)]
mod job_status_tests {
    use super::*;

    #[test]
    fn job_status_as_str() {
        assert_eq!(JobStatus::Pending.as_str(), "pending");
        assert_eq!(JobStatus::Running.as_str(), "running");
        assert_eq!(JobStatus::Succeeded.as_str(), "succeeded");
        assert_eq!(JobStatus::Failed.as_str(), "failed");
        assert_eq!(JobStatus::Cancelled.as_str(), "cancelled");
        assert_eq!(JobStatus::TimedOut.as_str(), "timed_out");
    }

    #[test]
    fn job_status_is_terminal() {
        assert!(!JobStatus::Pending.is_terminal());
        assert!(!JobStatus::Running.is_terminal());
        assert!(JobStatus::Succeeded.is_terminal());
        assert!(JobStatus::Failed.is_terminal());
        assert!(JobStatus::Cancelled.is_terminal());
        assert!(JobStatus::TimedOut.is_terminal());
    }

    #[test]
    fn job_status_from_workflow_status() {
        assert_eq!(JobStatus::from(WorkflowStatus::Pending), JobStatus::Pending);
        assert_eq!(JobStatus::from(WorkflowStatus::Running), JobStatus::Running);
        assert_eq!(
            JobStatus::from(WorkflowStatus::Succeeded),
            JobStatus::Succeeded
        );
        assert_eq!(JobStatus::from(WorkflowStatus::Failed), JobStatus::Failed);
        assert_eq!(
            JobStatus::from(WorkflowStatus::Cancelled),
            JobStatus::Cancelled
        );
    }
}

#[cfg(test)]
mod job_backend_kind_tests {
    use super::*;

    #[test]
    fn job_backend_kind_as_str() {
        assert_eq!(JobBackendKind::InMemory.as_str(), "in_memory");
        assert_eq!(JobBackendKind::Persistent.as_str(), "persistent");
    }

    #[test]
    fn job_backend_kind_parse_str() {
        assert_eq!(
            JobBackendKind::parse_str("in_memory").unwrap(),
            JobBackendKind::InMemory
        );
        assert_eq!(
            JobBackendKind::parse_str("persistent").unwrap(),
            JobBackendKind::Persistent
        );
        assert!(JobBackendKind::parse_str("invalid").is_err());
    }

    #[test]
    fn job_backend_kind_display() {
        assert_eq!(JobBackendKind::InMemory.to_string(), "in_memory");
        assert_eq!(JobBackendKind::Persistent.to_string(), "persistent");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_type_roundtrip() {
        for (variant, expected) in [
            (JobType::Standalone, "standalone"),
            (JobType::Workflow, "workflow"),
            (JobType::Cron, "cron"),
        ] {
            assert_eq!(variant.as_str(), expected);
            assert_eq!(JobType::parse_str(expected).unwrap(), variant);
        }
    }

    #[test]
    fn job_type_invalid() {
        assert!(JobType::parse_str("invalid").is_err());
        assert!(JobType::parse_str("STANDALONE").is_err());
        assert!(JobType::parse_str("").is_err());
    }

    #[test]
    fn job_type_display() {
        assert_eq!(JobType::Standalone.to_string(), "standalone");
        assert_eq!(JobType::Workflow.to_string(), "workflow");
        assert_eq!(JobType::Cron.to_string(), "cron");
    }

    #[test]
    fn job_record_for_test_defaults() {
        let record = JobRecord::for_test("j-1", "agent-1", "test job", "do something");
        assert_eq!(record.id.as_ref(), "j-1");
        assert_eq!(record.job_type, JobType::Standalone);
        assert_eq!(record.status, WorkflowStatus::Pending);
        assert_eq!(record.agent_id.as_ref(), "agent-1");
        assert_eq!(record.prompt, "do something");
        assert!(record.depends_on.is_empty());
        assert!(record.thread_id.is_none());
    }
}
