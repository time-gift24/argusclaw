//! crates/claw/src/workflow/repository.rs

use async_trait::async_trait;

use super::types::{
    JobId, JobRecord, StageId, StageRecord, WorkflowId, WorkflowRecord, WorkflowStatus,
};
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

    // Stage CRUD
    async fn create_stage(&self, stage: &StageRecord) -> Result<(), DbError>;
    async fn list_stages_by_workflow(
        &self,
        workflow_id: &WorkflowId,
    ) -> Result<Vec<StageRecord>, DbError>;
    async fn update_stage_status(
        &self,
        id: &StageId,
        status: WorkflowStatus,
    ) -> Result<(), DbError>;

    // Job CRUD
    async fn create_job(&self, job: &JobRecord) -> Result<(), DbError>;
    async fn list_jobs_by_stage(&self, stage_id: &StageId) -> Result<Vec<JobRecord>, DbError>;
    async fn update_job_status(
        &self,
        id: &JobId,
        status: WorkflowStatus,
        started_at: Option<&str>,
        finished_at: Option<&str>,
    ) -> Result<(), DbError>;
}
