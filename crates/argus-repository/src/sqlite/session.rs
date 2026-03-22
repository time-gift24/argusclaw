use async_trait::async_trait;

use crate::error::DbError;
use crate::traits::{SessionRepository, SessionSummaryRecord};
use crate::types::SessionRecord;
use argus_protocol::SessionId;

use super::{ArgusSqlite, DbResult};

#[async_trait]
impl SessionRepository for ArgusSqlite {
    async fn create_session(&self, name: &str) -> DbResult<SessionId> {
        let result = sqlx::query(
            "INSERT INTO sessions (name, created_at, updated_at) VALUES (?1, datetime('now'), datetime('now'))",
        )
        .bind(name)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(SessionId::new(result.last_insert_rowid()))
    }

    async fn get_session(&self, id: &SessionId) -> DbResult<Option<SessionRecord>> {
        let row =
            sqlx::query("SELECT id, name, created_at, updated_at FROM sessions WHERE id = ?1")
                .bind(id.inner())
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?;

        row.map(|r| self.map_session_record(r)).transpose()
    }

    async fn list_sessions(&self) -> DbResult<Vec<SessionSummaryRecord>> {
        let rows = sqlx::query(
            r#"
            SELECT s.id, s.name, s.updated_at, COUNT(t.id) as thread_count,
                   (SELECT t2.template_id FROM threads t2 WHERE t2.session_id = s.id LIMIT 1) as template_id,
                   (SELECT t3.provider_id FROM threads t3 WHERE t3.session_id = s.id LIMIT 1) as provider_id
            FROM sessions s
            LEFT JOIN threads t ON t.session_id = s.id
            GROUP BY s.id
            ORDER BY s.updated_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        rows.into_iter()
            .map(|r| self.map_session_summary_record(r))
            .collect()
    }

    async fn update_session(&self, id: &SessionId, name: &str) -> DbResult<()> {
        sqlx::query("UPDATE sessions SET name = ?2, updated_at = datetime('now') WHERE id = ?1")
            .bind(id.inner())
            .bind(name)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        Ok(())
    }

    async fn delete_session(&self, id: &SessionId) -> DbResult<bool> {
        let result = sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(id.inner())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        Ok(result.rows_affected() > 0)
    }

    async fn cleanup_old_sessions(&self, days: u32) -> DbResult<u64> {
        let result = sqlx::query(
            r#"
            DELETE FROM sessions WHERE id IN (
                SELECT s.id FROM sessions s
                WHERE COALESCE(
                    (SELECT MAX(t.updated_at) FROM threads t WHERE t.session_id = s.id),
                    s.updated_at
                ) < datetime('now', '-' || ?1 || ' days')
            )
            "#,
        )
        .bind(days as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        Ok(result.rows_affected())
    }
}

impl ArgusSqlite {
    fn map_session_record(&self, row: sqlx::sqlite::SqliteRow) -> DbResult<SessionRecord> {
        Ok(SessionRecord {
            id: SessionId::new(Self::get_column(&row, "id")?),
            name: Self::get_column(&row, "name")?,
            created_at: Self::get_column(&row, "created_at")?,
            updated_at: Self::get_column(&row, "updated_at")?,
        })
    }

    fn map_session_summary_record(
        &self,
        row: sqlx::sqlite::SqliteRow,
    ) -> DbResult<SessionSummaryRecord> {
        use argus_protocol::SessionId;

        Ok(SessionSummaryRecord {
            id: SessionId::new(Self::get_column::<i64>(&row, "id")?),
            name: Self::get_column(&row, "name")?,
            thread_count: Self::get_column::<i64>(&row, "thread_count")? as u64,
            template_id: Self::get_column::<Option<i64>>(&row, "template_id")?.map(|v| v as u64),
            provider_id: Self::get_column::<Option<i64>>(&row, "provider_id")?.map(|v| v as u64),
            updated_at: Self::get_column(&row, "updated_at")?,
        })
    }
}
