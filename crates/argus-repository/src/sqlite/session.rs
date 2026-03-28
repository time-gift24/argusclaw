//! SessionRepository implementation for SQLite.

use async_trait::async_trait;

use crate::error::DbError;
use crate::traits::{SessionRepository, SessionWithCount};
use crate::types::SessionRecord;
use argus_protocol::SessionId;

use super::{ArgusSqlite, DbResult};

#[async_trait]
impl SessionRepository for ArgusSqlite {
    async fn list_with_counts(&self) -> DbResult<Vec<SessionWithCount>> {
        let rows = sqlx::query(
            "SELECT s.id, s.name, s.created_at, s.updated_at, COUNT(t.id) as thread_count
             FROM sessions s LEFT JOIN threads t ON t.session_id = s.id
             GROUP BY s.id ORDER BY s.updated_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.into_iter()
            .map(|r| self.map_session_with_count(r))
            .collect()
    }

    async fn get(&self, id: &SessionId) -> DbResult<Option<SessionRecord>> {
        let row =
            sqlx::query("SELECT id, name, created_at, updated_at FROM sessions WHERE id = ?1")
                .bind(id.to_string())
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?;

        row.map(|r| self.map_session_record(&r)).transpose()
    }

    async fn create(&self, id: &SessionId, name: &str) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO sessions (id, name, created_at, updated_at)
             VALUES (?1, ?2, datetime('now'), datetime('now'))",
        )
        .bind(id.to_string())
        .bind(name)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    async fn rename(&self, id: &SessionId, name: &str) -> DbResult<bool> {
        let result = sqlx::query(
            "UPDATE sessions SET name = ?1, updated_at = CURRENT_TIMESTAMP WHERE id = ?2",
        )
        .bind(name)
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(result.rows_affected() > 0)
    }

    async fn delete(&self, id: &SessionId) -> DbResult<bool> {
        let result = sqlx::query("DELETE FROM sessions WHERE id = ?1")
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
    fn map_session_record(&self, row: &sqlx::sqlite::SqliteRow) -> DbResult<SessionRecord> {
        Ok(SessionRecord {
            id: SessionId::parse(&Self::get_column::<String>(row, "id")?).map_err(|e| {
                DbError::QueryFailed {
                    reason: format!("invalid session id: {e}"),
                }
            })?,
            name: Self::get_column(row, "name")?,
            created_at: Self::get_column(row, "created_at")?,
            updated_at: Self::get_column(row, "updated_at")?,
        })
    }

    fn map_session_with_count(&self, row: sqlx::sqlite::SqliteRow) -> DbResult<SessionWithCount> {
        let session = self.map_session_record(&row)?;
        let thread_count = Self::get_column::<i64>(&row, "thread_count")?;
        Ok(SessionWithCount {
            session,
            thread_count,
        })
    }
}
