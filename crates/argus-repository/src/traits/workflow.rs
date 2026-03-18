//! Workflow repository trait.

use async_trait::async_trait;

use crate::error::DbError;
use crate::types::{WorkflowId, WorkflowRecord, WorkflowStatus};

/// Repository for workflow persistence.
#[async_trait]
pub trait WorkflowRepository: Send + Sync {
    /// Create a new workflow.
    async fn create_workflow(&self, workflow: &WorkflowRecord) -> Result<(), DbError>;

    /// Get a workflow by ID.
    async fn get_workflow(&self, id: &WorkflowId) -> Result<Option<WorkflowRecord>, DbError>;

    /// Update workflow status.
    async fn update_workflow_status(
        &self,
        id: &WorkflowId,
        status: WorkflowStatus,
    ) -> Result<(), DbError>;

    /// List all workflows.
    async fn list_workflows(&self) -> Result<Vec<WorkflowRecord>, DbError>;

    /// Delete a workflow.
    async fn delete_workflow(&self, id: &WorkflowId) -> Result<bool, DbError>;
}
