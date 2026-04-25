//! AgentRunRepository implementation for SQLite.

use async_trait::async_trait;

use crate::error::DbError;
use crate::traits::AgentRunRepository;
use crate::types::{AgentRunId, AgentRunRecord, AgentRunStatus};

use super::{ArgusSqlite, DbResult};

#[async_trait]
impl AgentRunRepository for ArgusSqlite {
    async fn insert_agent_run(&self, record: &AgentRunRecord) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO agent_runs (
                 id, agent_id, session_id, thread_id, prompt, status, result, error,
                 created_at, updated_at, completed_at
             )
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        )
        .bind(record.id.to_string())
        .bind(record.agent_id.inner())
        .bind(record.session_id.to_string())
        .bind(record.thread_id.to_string())
        .bind(&record.prompt)
        .bind(record.status.as_str())
        .bind(&record.result)
        .bind(&record.error)
        .bind(&record.created_at)
        .bind(&record.updated_at)
        .bind(&record.completed_at)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    async fn get_agent_run(&self, id: &AgentRunId) -> DbResult<Option<AgentRunRecord>> {
        let row = sqlx::query(
            "SELECT id, agent_id, session_id, thread_id, prompt, status, result, error,
                    created_at, updated_at, completed_at
             FROM agent_runs
             WHERE id = ?1",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(|row| self.map_agent_run_record(row)).transpose()
    }

    async fn update_agent_run_status(
        &self,
        id: &AgentRunId,
        status: AgentRunStatus,
        result: Option<&str>,
        error: Option<&str>,
        completed_at: Option<&str>,
        updated_at: &str,
    ) -> DbResult<()> {
        sqlx::query(
            "UPDATE agent_runs
             SET status = ?1,
                 result = ?2,
                 error = ?3,
                 completed_at = ?4,
                 updated_at = ?5
             WHERE id = ?6",
        )
        .bind(status.as_str())
        .bind(result)
        .bind(error)
        .bind(completed_at)
        .bind(updated_at)
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    async fn delete_agent_run(&self, id: &AgentRunId) -> DbResult<bool> {
        let result = sqlx::query("DELETE FROM agent_runs WHERE id = ?1")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(result.rows_affected() > 0)
    }
}

impl ArgusSqlite {
    fn map_agent_run_record(&self, row: sqlx::sqlite::SqliteRow) -> DbResult<AgentRunRecord> {
        let status_text: String = Self::get_column(&row, "status")?;
        let status = AgentRunStatus::parse(&status_text).ok_or_else(|| DbError::QueryFailed {
            reason: format!("invalid agent run status: {status_text}"),
        })?;
        let run_id = AgentRunId::parse(&Self::get_column::<String>(&row, "id")?).map_err(|e| {
            DbError::QueryFailed {
                reason: format!("invalid agent run id: {e}"),
            }
        })?;
        let session_id =
            argus_protocol::SessionId::parse(&Self::get_column::<String>(&row, "session_id")?)
                .map_err(|e| DbError::QueryFailed {
                    reason: format!("invalid session id: {e}"),
                })?;
        let thread_id =
            argus_protocol::ThreadId::parse(&Self::get_column::<String>(&row, "thread_id")?)
                .map_err(|e| DbError::QueryFailed {
                    reason: format!("invalid thread id: {e}"),
                })?;

        Ok(AgentRunRecord {
            id: run_id,
            agent_id: argus_protocol::AgentId::new(Self::get_column(&row, "agent_id")?),
            session_id,
            thread_id,
            prompt: Self::get_column(&row, "prompt")?,
            status,
            result: Self::get_column(&row, "result")?,
            error: Self::get_column(&row, "error")?,
            created_at: Self::get_column(&row, "created_at")?,
            updated_at: Self::get_column(&row, "updated_at")?,
            completed_at: Self::get_column(&row, "completed_at")?,
        })
    }
}
