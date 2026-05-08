//! Job persistence types.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use super::AgentId;
use argus_protocol::{ThreadId, TokenUsage};

/// Unique identifier for a job.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct JobId(String);

impl JobId {
    /// Creates a new job ID.
    ///
    /// # Panics
    /// Panics in debug mode if `id` is empty.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        debug_assert!(!id.is_empty(), "JobId cannot be empty");
        Self(id)
    }
}

impl AsRef<str> for JobId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl FromStr for JobId {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(s))
    }
}

/// The execution status of a job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobStatus {
    /// Job is pending execution.
    Pending,
    /// Job has been admitted to the thread pool and is waiting for execution.
    Queued,
    /// Job is currently running.
    Running,
    /// Job completed successfully.
    Succeeded,
    /// Job failed.
    Failed,
    /// Job was cancelled.
    Cancelled,
    /// Job is paused.
    Paused,
}

impl JobStatus {
    /// Returns the string representation of this status.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Paused => "paused",
            Self::Cancelled => "cancelled",
        }
    }

    /// Parses a job status from a string.
    pub fn parse_str(s: &str) -> Result<Self, String> {
        match s {
            "pending" => Ok(Self::Pending),
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "paused" => Ok(Self::Paused),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(format!("invalid job status: {s}")),
        }
    }
}

impl fmt::Display for JobStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TryFrom<&str> for JobStatus {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse_str(value)
    }
}

/// Context for a scheduled message job.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledMessageContext {
    pub target_session_id: String,
    #[serde(default = "default_enabled")]
    pub enabled: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timezone: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<String>,
}

fn default_enabled() -> bool {
    true
}

impl ScheduledMessageContext {
    #[must_use]
    pub fn new(target_session_id: impl Into<String>) -> Self {
        Self {
            target_session_id: target_session_id.into(),
            enabled: default_enabled(),
            timezone: None,
            last_error: None,
        }
    }
}

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
    /// Agent ID that handled the subagent work.
    pub agent_id: AgentId,
    /// Human-readable subagent name.
    pub agent_display_name: String,
    /// Subagent description.
    pub agent_description: String,
}

/// The kind of job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum JobType {
    /// Standalone job.
    Standalone,
    /// Scheduled cron job.
    Cron,
}

impl JobType {
    /// Returns the string representation of this job type.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Standalone => "standalone",
            Self::Cron => "cron",
        }
    }

    /// Parses a job type from a string.
    pub fn parse_str(s: &str) -> Result<Self, String> {
        match s {
            "standalone" => Ok(Self::Standalone),
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
    pub status: JobStatus,
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
    /// Parent job ID (for subagent-dispatched jobs).
    pub parent_job_id: Option<JobId>,
    /// Job execution result (set when job completes).
    pub result: Option<JobResult>,
}

#[cfg(test)]
impl JobRecord {
    /// Creates a minimal job record for testing.
    #[must_use]
    pub fn for_test(id: &str, agent_id: i64, name: &str, prompt: &str) -> Self {
        Self {
            id: JobId::new(id),
            job_type: JobType::Standalone,
            name: name.to_string(),
            status: JobStatus::Pending,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn job_id_from_str() {
        let id = JobId::from_str("job-123").unwrap();
        assert_eq!(id.as_ref(), "job-123");
    }

    #[test]
    fn job_id_new_preserves_string_value() {
        let id = JobId::new("job-xyz");
        assert_eq!(id.as_ref(), "job-xyz");
    }

    #[test]
    fn job_status_as_str() {
        assert_eq!(JobStatus::Pending.as_str(), "pending");
        assert_eq!(JobStatus::Queued.as_str(), "queued");
        assert_eq!(JobStatus::Running.as_str(), "running");
        assert_eq!(JobStatus::Succeeded.as_str(), "succeeded");
        assert_eq!(JobStatus::Failed.as_str(), "failed");
        assert_eq!(JobStatus::Paused.as_str(), "paused");
        assert_eq!(JobStatus::Cancelled.as_str(), "cancelled");
    }

    #[test]
    fn job_status_from_str_valid() {
        assert_eq!(JobStatus::parse_str("pending").unwrap(), JobStatus::Pending);
        assert_eq!(JobStatus::parse_str("queued").unwrap(), JobStatus::Queued);
        assert_eq!(JobStatus::parse_str("running").unwrap(), JobStatus::Running);
        assert_eq!(
            JobStatus::parse_str("succeeded").unwrap(),
            JobStatus::Succeeded
        );
        assert_eq!(JobStatus::parse_str("failed").unwrap(), JobStatus::Failed);
        assert_eq!(JobStatus::parse_str("paused").unwrap(), JobStatus::Paused);
        assert_eq!(
            JobStatus::parse_str("cancelled").unwrap(),
            JobStatus::Cancelled
        );
    }

    #[test]
    fn job_status_parses_paused() {
        assert_eq!(JobStatus::parse_str("paused").unwrap(), JobStatus::Paused);
        assert_eq!(JobStatus::Paused.as_str(), "paused");
    }

    #[test]
    fn scheduled_message_context_round_trips() {
        let context = ScheduledMessageContext {
            target_session_id: "session-1".to_string(),
            enabled: true,
            timezone: Some("Asia/Shanghai".to_string()),
            last_error: None,
        };

        let json = serde_json::to_string(&context).unwrap();
        let restored: ScheduledMessageContext = serde_json::from_str(&json).unwrap();

        assert_eq!(restored.target_session_id, "session-1");
        assert!(restored.enabled);
        assert_eq!(restored.timezone.as_deref(), Some("Asia/Shanghai"));
    }

    #[test]
    fn job_status_from_str_invalid() {
        assert!(JobStatus::parse_str("invalid").is_err());
    }

    #[test]
    fn job_type_rejects_legacy_workflow_kind() {
        assert!(JobType::parse_str("workflow").is_err());
    }
}
