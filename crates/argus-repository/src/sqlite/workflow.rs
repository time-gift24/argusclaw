//! WorkflowRepository implementation for SQLite.

use async_trait::async_trait;

use crate::error::DbError;
use crate::traits::WorkflowRepository;
use crate::types::{WorkflowId, WorkflowRecord, WorkflowStatus};

use super::{ArgusSqlite, DbResult};

#[async_trait]
impl WorkflowRepository for ArgusSqlite {
    async fn create_workflow(&self, workflow: &WorkflowRecord) -> DbResult<()> {
        sqlx::query("INSERT INTO workflows (id, name, status) VALUES (?1, ?2, ?3)")
            .bind(workflow.id.as_ref())
            .bind(&workflow.name)
            .bind(workflow.status.as_str())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(())
    }

    async fn get_workflow(&self, id: &WorkflowId) -> DbResult<Option<WorkflowRecord>> {
        let row = sqlx::query("SELECT id, name, status FROM workflows WHERE id = ?1")
            .bind(id.as_ref())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        row.map(|r| self.map_workflow_record(r)).transpose()
    }

    async fn update_workflow_status(
        &self,
        id: &WorkflowId,
        status: WorkflowStatus,
    ) -> DbResult<()> {
        let result = sqlx::query("UPDATE workflows SET status = ?1 WHERE id = ?2")
            .bind(status.as_str())
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

    async fn list_workflows(&self) -> DbResult<Vec<WorkflowRecord>> {
        let rows = sqlx::query("SELECT id, name, status FROM workflows ORDER BY name")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        rows.into_iter()
            .map(|r| self.map_workflow_record(r))
            .collect()
    }

    async fn delete_workflow(&self, id: &WorkflowId) -> DbResult<bool> {
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

impl ArgusSqlite {
    fn map_workflow_record(&self, row: sqlx::sqlite::SqliteRow) -> DbResult<WorkflowRecord> {
        Ok(WorkflowRecord {
            id: WorkflowId::new(Self::get_column::<String>(&row, "id")?),
            name: Self::get_column(&row, "name")?,
            status: WorkflowStatus::parse_str(&Self::get_column::<String>(&row, "status")?)
                .map_err(|e| DbError::QueryFailed { reason: e })?,
        })
    }
}
