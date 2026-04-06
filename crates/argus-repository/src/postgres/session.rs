//! SessionRepository implementation for PostgreSQL with owner-aware queries.

use crate::error::DbError;
use crate::traits::{SessionRepository, SessionWithCount, UserSessionRepository};
use crate::types::SessionRecord;
use argus_protocol::SessionId;
use async_trait::async_trait;

use super::user::get_column;
use super::{ArgusPostgres, DbResult};

#[async_trait]
impl SessionRepository for ArgusPostgres {
    async fn list_with_counts(&self) -> DbResult<Vec<SessionWithCount>> {
        let rows = sqlx::query(
            "SELECT s.id, s.name, s.created_at, s.updated_at, COUNT(t.id) as thread_count \
             FROM sessions s LEFT JOIN threads t ON t.session_id = s.id \
             GROUP BY s.id ORDER BY s.updated_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.iter().map(|row| map_session_with_count(row)).collect()
    }

    async fn get(&self, id: &SessionId) -> DbResult<Option<SessionRecord>> {
        let row =
            sqlx::query("SELECT id, name, created_at, updated_at FROM sessions WHERE id = $1")
                .bind(id.to_string())
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?;

        row.map(|row| map_session_record(&row)).transpose()
    }

    async fn create(&self, id: &SessionId, name: &str) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO sessions (id, name, created_at, updated_at) \
             VALUES ($1, $2, NOW(), NOW())",
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
        let result = sqlx::query("UPDATE sessions SET name = $1, updated_at = NOW() WHERE id = $2")
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
        let result = sqlx::query("DELETE FROM sessions WHERE id = $1")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(result.rows_affected() > 0)
    }
}

#[async_trait]
impl UserSessionRepository for ArgusPostgres {
    async fn create_for_user(
        &self,
        id: &SessionId,
        name: &str,
        owner_user_id: i64,
    ) -> DbResult<()> {
        self.create_session_for_user(id, name, owner_user_id).await
    }

    async fn list_with_counts_for_user(
        &self,
        owner_user_id: i64,
    ) -> DbResult<Vec<SessionWithCount>> {
        self.list_sessions_for_user(owner_user_id).await
    }

    async fn user_owns_session(
        &self,
        owner_user_id: i64,
        session_id: &SessionId,
    ) -> DbResult<bool> {
        let owns: Option<i64> = sqlx::query_scalar(
            "SELECT 1 FROM sessions WHERE id = $1 AND owner_user_id = $2 LIMIT 1",
        )
        .bind(session_id.to_string())
        .bind(owner_user_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(owns.is_some())
    }
}

impl ArgusPostgres {
    /// Create a session owned by a specific user.
    pub async fn create_session_for_user(
        &self,
        id: &SessionId,
        name: &str,
        owner_user_id: i64,
    ) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO sessions (id, name, owner_user_id, created_at, updated_at) \
             VALUES ($1, $2, $3, NOW(), NOW())",
        )
        .bind(id.to_string())
        .bind(name)
        .bind(owner_user_id)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    /// List sessions belonging to a specific user with thread counts.
    pub async fn list_sessions_for_user(
        &self,
        owner_user_id: i64,
    ) -> DbResult<Vec<SessionWithCount>> {
        let rows = sqlx::query(
            "SELECT s.id, s.name, s.created_at, s.updated_at, COUNT(t.id) as thread_count \
             FROM sessions s LEFT JOIN threads t ON t.session_id = s.id \
             WHERE s.owner_user_id = $1 \
             GROUP BY s.id ORDER BY s.updated_at DESC",
        )
        .bind(owner_user_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.iter().map(|row| map_session_with_count(row)).collect()
    }
}

fn map_session_record(row: &sqlx::postgres::PgRow) -> DbResult<SessionRecord> {
    Ok(SessionRecord {
        id: SessionId::parse(&get_column::<String>(&row, "id")?).map_err(|e| {
            DbError::QueryFailed {
                reason: format!("invalid session id: {e}"),
            }
        })?,
        name: get_column(&row, "name")?,
        created_at: get_column(&row, "created_at")?,
        updated_at: get_column(&row, "updated_at")?,
    })
}

fn map_session_with_count(row: &sqlx::postgres::PgRow) -> DbResult<SessionWithCount> {
    let thread_count = get_column::<i64>(&row, "thread_count")?;
    let session = map_session_record(row)?;
    Ok(SessionWithCount {
        session,
        thread_count,
    })
}
