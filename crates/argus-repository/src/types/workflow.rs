//! Workflow persistence types.

use std::fmt;
use std::str::FromStr;

use argus_protocol::{AgentId, ThreadId};
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
impl FromStr for WorkflowId {
    type Err = std::convert::Infallible;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(Self::new(s))
    }
}

/// Unique identifier for a job (alias for WorkflowId for use in job records).
pub type JobId = WorkflowId;

/// Unique identifier for a workflow template.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WorkflowTemplateId(String);

impl WorkflowTemplateId {
    /// Creates a new workflow template ID.
    ///
    /// # Panics
    /// Panics in debug mode if `id` is empty.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        debug_assert!(!id.is_empty(), "WorkflowTemplateId cannot be empty");
        Self(id)
    }
}

impl AsRef<str> for WorkflowTemplateId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for WorkflowTemplateId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

impl FromStr for WorkflowTemplateId {
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
    /// Workflow has been admitted to the thread pool and is waiting for execution.
    Queued,
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
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Succeeded => "succeeded",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Parses a workflow status from a string.
    pub fn parse_str(s: &str) -> Result<Self, String> {
        match s {
            "pending" => Ok(Self::Pending),
            "queued" => Ok(Self::Queued),
            "running" => Ok(Self::Running),
            "succeeded" => Ok(Self::Succeeded),
            "failed" => Ok(Self::Failed),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(format!("invalid workflow status: {s}")),
        }
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

/// Workflow template stored in database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowTemplateRecord {
    pub id: WorkflowTemplateId,
    pub name: String,
    pub version: i64,
    pub description: String,
}

/// Workflow template node stored in database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowTemplateNodeRecord {
    pub template_id: WorkflowTemplateId,
    pub node_key: String,
    pub name: String,
    pub agent_id: AgentId,
    pub prompt: String,
    pub context: Option<String>,
    pub depends_on_keys: Vec<String>,
}

/// Workflow execution header stored in database.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowExecutionRecord {
    pub id: WorkflowId,
    pub name: String,
    pub status: WorkflowStatus,
    pub template_id: Option<WorkflowTemplateId>,
    pub template_version: Option<i64>,
    pub initiating_thread_id: Option<ThreadId>,
}

/// Workflow execution node helper used when instantiating workflows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowExecutionNodeRecord {
    pub node_key: String,
    pub name: String,
    pub agent_id: AgentId,
    pub prompt: String,
    pub context: Option<String>,
    pub depends_on_keys: Vec<String>,
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
        let id = JobId::new("job-xyz");
        assert_eq!(id.as_ref(), "job-xyz");
    }

    #[test]
    fn workflow_status_as_str() {
        assert_eq!(WorkflowStatus::Pending.as_str(), "pending");
        assert_eq!(WorkflowStatus::Queued.as_str(), "queued");
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
            WorkflowStatus::parse_str("queued").unwrap(),
            WorkflowStatus::Queued
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
    }

    #[test]
    fn workflow_template_node_keeps_depends_on_keys() {
        let node = WorkflowTemplateNodeRecord {
            template_id: WorkflowTemplateId::new("tpl-1"),
            node_key: "summarize".to_string(),
            name: "Summarize".to_string(),
            agent_id: AgentId::new(7),
            prompt: "Summarize the repo".to_string(),
            context: None,
            depends_on_keys: vec!["collect".to_string()],
        };

        assert_eq!(node.depends_on_keys, vec!["collect"]);
    }

    #[test]
    fn workflow_execution_node_keeps_node_key() {
        let execution = WorkflowExecutionRecord {
            id: WorkflowId::new("workflow-1"),
            name: "demo".to_string(),
            status: WorkflowStatus::Pending,
            template_id: Some(WorkflowTemplateId::new("tpl-1")),
            template_version: Some(1),
            initiating_thread_id: Some(ThreadId::new()),
        };

        let node = WorkflowExecutionNodeRecord {
            node_key: "collect".to_string(),
            name: "Collect".to_string(),
            agent_id: AgentId::new(7),
            prompt: "Collect context".to_string(),
            context: None,
            depends_on_keys: vec![],
        };

        assert_eq!(execution.template_version, Some(1));
        assert_eq!(node.node_key, "collect");
    }
}
