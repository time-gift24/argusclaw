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

/// Unique identifier for a workflow stage.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct StageId(String);

impl StageId {
    /// Creates a new stage ID.
    ///
    /// # Panics
    /// Panics in debug mode if `id` is empty.
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        let id = id.into();
        debug_assert!(!id.is_empty(), "StageId cannot be empty");
        Self(id)
    }
}

impl AsRef<str> for StageId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for StageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
    }
}

/// Parses a stage ID from a string.
///
/// This implementation is intentionally infallible to match the behavior of `StageId::new()`.
/// If validation is needed in the future, use `StageId::try_from_str()` instead.
impl FromStr for StageId {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workflow_id_from_str() {
        let id = WorkflowId::from_str("workflow-123").unwrap();
        assert_eq!(id.as_ref(), "workflow-123");
    }

    #[test]
    fn stage_id_display() {
        let id = StageId::new("stage-abc");
        assert_eq!(id.to_string(), "stage-abc");
    }

    #[test]
    fn job_id_from_string() {
        let id = JobId::new("job-xyz".to_string());
        assert_eq!(id.as_ref(), "job-xyz");
    }
}
