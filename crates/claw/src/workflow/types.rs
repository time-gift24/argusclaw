//! Workflow domain types.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

/// Unique identifier for a workflow.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkflowId(String);

impl WorkflowId {
    /// Creates a new workflow ID.
    ///
    /// # Panics
    /// Panics in debug mode if `id` is empty.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        debug_assert!(!id.is_empty(), "WorkflowId cannot be empty");
        Self(id)
    }
}

impl AsRef<str> for WorkflowId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WorkflowId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

/// Parses a workflow ID from a string.
///
/// This implementation is intentionally infallible to match the behavior of `WorkflowId::new()`.
/// If validation is needed in the future, use `WorkflowId::try_from_str()` instead.
impl FromStr for WorkflowId {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(s))
    }
}

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

/// Parses a job ID from a string.
///
/// This implementation is intentionally infallible to match the behavior of `JobId::new()`.
/// If validation is needed in the future, use `JobId::try_from_str()` instead.
impl FromStr for JobId {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(s))
    }
}

/// The execution status of a workflow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WorkflowStatus {
    /// Workflow is pending execution.
    Pending,
    /// Workflow is currently running.
    Running,
    /// Workflow completed successfully.
    Succeeded,
    /// Workflow failed.
    Failed,
    /// Workflow was cancelled.
    Cancelled,
}

impl WorkflowStatus {
    /// Returns the string representation of this status.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Parses a workflow status from a string.
    ///
    /// # Errors
    /// Returns an error if the string is not a valid status representation.
    pub fn parse_str(s: &str) -> Result<Self, String> {
        match s {
            "pending" => Ok(Self::Pending),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(format!("invalid workflow status: {s}")),
        }
    }

    /// Returns true if this is a terminal status (cannot transition to another state).
    #[must_use]
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed | Self::Cancelled)
    }
}

impl fmt::Display for WorkflowStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl TryFrom<&str> for WorkflowStatus {
    type Error = String;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Self::parse_str(value)
    }
}

/// Workflow record stored in database.
pub struct WorkflowRecord {
    pub id: WorkflowId,
    pub name: String,
    pub status: WorkflowStatus,
}

#[cfg(test)]
impl WorkflowRecord {
    /// Creates a test workflow record.
    #[must_use]
    pub fn for_test(id: &str, name: &str) -> Self {
        Self {
            id: WorkflowId::new(id),
            name: name.to_string(),
            status: WorkflowStatus::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_id_from_str() {
        let id = WorkflowId::from_str("workflow-123").unwrap();
        assert_eq!(id.as_ref(), "workflow-123");
    }

    #[test]
    fn job_id_from_string() {
        let id = JobId::new("job-xyz".to_string());
        assert_eq!(id.as_ref(), "job-xyz");
    }

    #[test]
    fn workflow_status_as_str() {
        assert_eq!(WorkflowStatus::Pending.as_str(), "pending");
        assert_eq!(WorkflowStatus::Running.as_str(), "running");
        assert_eq!(WorkflowStatus::Succeeded.as_str(), "succeeded");
        assert_eq!(WorkflowStatus::Failed.as_str(), "failed");
        assert_eq!(WorkflowStatus::Cancelled.as_str(), "cancelled");
    }

    #[test]
    fn workflow_status_from_str_valid() {
        assert_eq!(
            WorkflowStatus::parse_str("pending").unwrap(),
            WorkflowStatus::Pending
        );
        assert_eq!(
            WorkflowStatus::parse_str("running").unwrap(),
            WorkflowStatus::Running
        );
        assert_eq!(
            WorkflowStatus::parse_str("succeeded").unwrap(),
            WorkflowStatus::Succeeded
        );
        assert_eq!(
            WorkflowStatus::parse_str("failed").unwrap(),
            WorkflowStatus::Failed
        );
        assert_eq!(
            WorkflowStatus::parse_str("cancelled").unwrap(),
            WorkflowStatus::Cancelled
        );
    }

    #[test]
    fn workflow_status_from_str_invalid() {
        assert!(WorkflowStatus::parse_str("invalid").is_err());
        assert!(WorkflowStatus::parse_str("PENDING").is_err());
        assert!(WorkflowStatus::parse_str("").is_err());
    }

    #[test]
    fn workflow_status_display() {
        assert_eq!(WorkflowStatus::Pending.to_string(), "pending");
        assert_eq!(WorkflowStatus::Running.to_string(), "running");
        assert_eq!(WorkflowStatus::Succeeded.to_string(), "succeeded");
        assert_eq!(WorkflowStatus::Failed.to_string(), "failed");
        assert_eq!(WorkflowStatus::Cancelled.to_string(), "cancelled");
    }
}
