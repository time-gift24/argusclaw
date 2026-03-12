//! crates/claw/src/workflow/repository.rs

use async_trait::async_trait;

use super::types::{WorkflowId, WorkflowRecord, WorkflowStatus};
use crate::db::DbError;

/// Repository for workflow persistence.
#[async_trait]
pub trait WorkflowRepository: Send + Sync {
    // Workflow CRUD
    async fn create_workflow(&self, workflow: &WorkflowRecord) -> Result<(), DbError>;
    async fn get_workflow(&self, id: &WorkflowId) -> Result<Option<WorkflowRecord>, DbError>;
    async fn update_workflow_status(
        &self,
        id: &WorkflowId,
        status: WorkflowStatus,
    ) -> Result<(), DbError>;
    async fn list_workflows(&self) -> Result<Vec<WorkflowRecord>, DbError>;
    async fn delete_workflow(&self, id: &WorkflowId) -> Result<bool, DbError>;
}
