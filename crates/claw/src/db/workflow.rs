//! SQLite implementation of the workflow repository (lightweight grouping only).

use async_trait::async_trait;
use sqlx::{Row, SqlitePool};

use crate::db::DbError;
use crate::workflow::{WorkflowId, WorkflowRecord, WorkflowRepository, WorkflowStatus};

/// SQLite-backed workflow repository (for lightweight grouping only).
pub struct SqliteWorkflowRepository {
    pool: SqlitePool,
}

impl SqliteWorkflowRepository {
    /// Create a new SQLite workflow repository.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Parse a workflow status from a string.
    fn parse_status(s: String) -> Result<WorkflowStatus, DbError> {
        WorkflowStatus::parse_str(&s).map_err(|_| DbError::QueryFailed {
            reason: format!("invalid workflow status: {s}"),
        })
    }

    /// Map a database row to a WorkflowRecord.
    fn map_workflow(row: sqlx::sqlite::SqliteRow) -> Result<WorkflowRecord, DbError> {
        Ok(WorkflowRecord {
            id: WorkflowId::new(row.get::<String, _>("id")),
            name: row.get::<String, _>("name"),
            status: Self::parse_status(row.get::<String, _>("status"))?,
        })
    }

    /// Convert a WorkflowStatus to its string representation.
    fn status_as_str(status: WorkflowStatus) -> &'static str {
        status.as_str()
    }
}

#[async_trait]
impl WorkflowRepository for SqliteWorkflowRepository {
    /// Create a new workflow.
    async fn create_workflow(&self, workflow: &WorkflowRecord) -> Result<(), DbError> {
        sqlx::query("INSERT INTO workflows (id, name, status) VALUES (?1, ?2, ?3)")
            .bind(workflow.id.as_ref())
            .bind(&workflow.name)
            .bind(Self::status_as_str(workflow.status))
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(())
    }

    /// Get a workflow by ID.
    async fn get_workflow(&self, id: &WorkflowId) -> Result<Option<WorkflowRecord>, DbError> {
        let row = sqlx::query("SELECT id, name, status FROM workflows WHERE id = ?1")
            .bind(id.as_ref())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        row.map(Self::map_workflow).transpose()
    }

    /// Update workflow status.
    async fn update_workflow_status(
        &self,
        id: &WorkflowId,
        status: WorkflowStatus,
    ) -> Result<(), DbError> {
        let result = sqlx::query("UPDATE workflows SET status = ?1 WHERE id = ?2")
            .bind(Self::status_as_str(status))
            .bind(id.as_ref())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        if result.rows_affected() == 0 {
            return Err(DbError::QueryFailed {
                reason: format!("workflow not found: {}", id),
            });
        }

        Ok(())
    }

    /// List all workflows.
    async fn list_workflows(&self) -> Result<Vec<WorkflowRecord>, DbError> {
        let rows = sqlx::query("SELECT id, name, status FROM workflows ORDER BY name")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        rows.into_iter().map(Self::map_workflow).collect()
    }

    /// Delete a workflow.
    async fn delete_workflow(&self, id: &WorkflowId) -> Result<bool, DbError> {
        let result = sqlx::query("DELETE FROM workflows WHERE id = ?1")
            .bind(id.as_ref())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(result.rows_affected() > 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::sqlite::{connect, migrate};

    async fn create_test_pool() -> SqlitePool {
        let pool = connect("sqlite::memory:").await.expect("failed to connect");
        migrate(&pool).await.expect("failed to migrate");
        pool
    }

    #[tokio::test]
    async fn create_and_get_workflow() {
        let pool = create_test_pool().await;
        let repo = SqliteWorkflowRepository::new(pool);

        let workflow = WorkflowRecord::for_test("wf-1", "Test Workflow");
        repo.create_workflow(&workflow).await.unwrap();

        let retrieved = repo.get_workflow(&workflow.id).await.unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.id, workflow.id);
        assert_eq!(retrieved.name, workflow.name);
        assert_eq!(retrieved.status, workflow.status);
    }

    #[tokio::test]
    async fn list_workflows() {
        let pool = create_test_pool().await;
        let repo = SqliteWorkflowRepository::new(pool);

        let wf1 = WorkflowRecord::for_test("wf-1", "Alpha Workflow");
        let wf2 = WorkflowRecord::for_test("wf-2", "Beta Workflow");
        let wf3 = WorkflowRecord::for_test("wf-3", "Gamma Workflow");

        repo.create_workflow(&wf1).await.unwrap();
        repo.create_workflow(&wf2).await.unwrap();
        repo.create_workflow(&wf3).await.unwrap();

        let workflows = repo.list_workflows().await.unwrap();
        assert_eq!(workflows.len(), 3);
        assert_eq!(workflows[0].name, "Alpha Workflow");
        assert_eq!(workflows[1].name, "Beta Workflow");
        assert_eq!(workflows[2].name, "Gamma Workflow");
    }

    #[tokio::test]
    async fn update_workflow_status() {
        let pool = create_test_pool().await;
        let repo = SqliteWorkflowRepository::new(pool);

        let workflow = WorkflowRecord::for_test("wf-1", "Test Workflow");
        repo.create_workflow(&workflow).await.unwrap();

        repo.update_workflow_status(&workflow.id, WorkflowStatus::Running)
            .await
            .unwrap();

        let retrieved = repo.get_workflow(&workflow.id).await.unwrap().unwrap();
        assert_eq!(retrieved.status, WorkflowStatus::Running);
    }

    #[tokio::test]
    async fn delete_workflow() {
        let pool = create_test_pool().await;
        let repo = SqliteWorkflowRepository::new(pool);

        let workflow = WorkflowRecord::for_test("wf-1", "Test Workflow");
        repo.create_workflow(&workflow).await.unwrap();

        let deleted = repo.delete_workflow(&workflow.id).await.unwrap();
        assert!(deleted);

        let retrieved = repo.get_workflow(&workflow.id).await.unwrap();
        assert!(retrieved.is_none());
    }
}
