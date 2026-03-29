//! Workflow repository trait.

use async_trait::async_trait;

use argus_protocol::ThreadId;

use crate::error::DbError;
use crate::types::{
    WorkflowId, WorkflowProgressRecord, WorkflowRecord, WorkflowStatus, WorkflowTemplateId,
    WorkflowTemplateNodeRecord, WorkflowTemplateRecord,
};

/// Repository for workflow persistence.
#[async_trait]
pub trait WorkflowRepository: Send + Sync {
    /// Create a workflow template version.
    async fn create_workflow_template(
        &self,
        template: &WorkflowTemplateRecord,
    ) -> Result<(), DbError>;

    /// Read a workflow template version.
    async fn get_workflow_template(
        &self,
        id: &WorkflowTemplateId,
        version: i64,
    ) -> Result<Option<WorkflowTemplateRecord>, DbError>;

    /// Update a workflow template version.
    async fn update_workflow_template(
        &self,
        template: &WorkflowTemplateRecord,
    ) -> Result<(), DbError>;

    /// List all workflow template versions.
    async fn list_workflow_templates(&self) -> Result<Vec<WorkflowTemplateRecord>, DbError>;

    /// Delete a workflow template version.
    async fn delete_workflow_template(
        &self,
        id: &WorkflowTemplateId,
        version: i64,
    ) -> Result<bool, DbError>;

    /// Create a workflow template node.
    async fn create_workflow_template_node(
        &self,
        node: &WorkflowTemplateNodeRecord,
    ) -> Result<(), DbError>;

    /// Read a workflow template node.
    async fn get_workflow_template_node(
        &self,
        template_id: &WorkflowTemplateId,
        version: i64,
        node_key: &str,
    ) -> Result<Option<WorkflowTemplateNodeRecord>, DbError>;

    /// List nodes for a workflow template version.
    async fn list_workflow_template_nodes(
        &self,
        template_id: &WorkflowTemplateId,
        version: i64,
    ) -> Result<Vec<WorkflowTemplateNodeRecord>, DbError>;

    /// Create a workflow execution header.
    async fn create_workflow_execution(&self, workflow: &WorkflowRecord) -> Result<(), DbError>;

    /// Read a workflow execution header.
    async fn get_workflow_execution(
        &self,
        id: &WorkflowId,
    ) -> Result<Option<WorkflowRecord>, DbError>;

    /// Update the status of a workflow execution.
    async fn update_workflow_execution_status(
        &self,
        id: &WorkflowId,
        status: WorkflowStatus,
    ) -> Result<(), DbError>;

    /// List workflow executions.
    async fn list_workflow_executions(&self) -> Result<Vec<WorkflowRecord>, DbError>;

    /// List workflow executions initiated by a thread.
    async fn list_workflow_executions_by_initiating_thread(
        &self,
        thread_id: &ThreadId,
    ) -> Result<Vec<WorkflowRecord>, DbError>;

    /// Delete a workflow execution.
    async fn delete_workflow_execution(&self, id: &WorkflowId) -> Result<bool, DbError>;

    /// Read aggregated workflow progress.
    async fn get_workflow_progress(
        &self,
        id: &WorkflowId,
    ) -> Result<Option<WorkflowProgressRecord>, DbError>;

    /// Backwards-compatible alias for execution creation.
    async fn create_workflow(&self, workflow: &WorkflowRecord) -> Result<(), DbError> {
        self.create_workflow_execution(workflow).await
    }

    /// Backwards-compatible alias for execution lookup.
    async fn get_workflow(&self, id: &WorkflowId) -> Result<Option<WorkflowRecord>, DbError> {
        self.get_workflow_execution(id).await
    }

    /// Backwards-compatible alias for execution status updates.
    async fn update_workflow_status(
        &self,
        id: &WorkflowId,
        status: WorkflowStatus,
    ) -> Result<(), DbError> {
        self.update_workflow_execution_status(id, status).await
    }

    /// Backwards-compatible alias for listing executions.
    async fn list_workflows(&self) -> Result<Vec<WorkflowRecord>, DbError> {
        self.list_workflow_executions().await
    }

    /// Backwards-compatible alias for deleting a workflow execution.
    async fn delete_workflow(&self, id: &WorkflowId) -> Result<bool, DbError> {
        self.delete_workflow_execution(id).await
    }
}
