//! Workflow domain types.
//!
//! This module re-exports types from `argus_repository` for backward compatibility.

// Re-export types from argus_repository::types
pub use argus_repository::types::{
    JobId, WorkflowId, WorkflowRecord, WorkflowStatus,
};

#[cfg(test)]
mod tests {
    use super::*;
    use std::str::FromStr;

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
