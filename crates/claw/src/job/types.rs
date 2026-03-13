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
