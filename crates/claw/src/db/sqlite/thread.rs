//! SQLite implementation of ThreadRepository.

use async_trait::async_trait;
use sqlx::{Row, SqlitePool};

use crate::db::DbError;
use crate::db::thread::{MessageId, MessageRecord, ThreadRecord, ThreadRepository};
use crate::protocol::ThreadId;

/// SQLite-backed thread repository.
#[derive(Clone)]
pub struct SqliteThreadRepository {
    pool: SqlitePool,
}

impl SqliteThreadRepository {
    /// Create a new SQLite thread repository.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Helper to get a column value with consistent error mapping.
    fn get<T>(row: &sqlx::sqlite::SqliteRow, col: &str) -> Result<T, DbError>
    where
        T: for<'r> sqlx::decode::Decode<'r, sqlx::Sqlite> + sqlx::types::Type<sqlx::Sqlite>,
    {
        row.try_get(col).map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })
    }

    fn map_thread_record(row: sqlx::sqlite::SqliteRow) -> Result<ThreadRecord, DbError> {
        Ok(ThreadRecord {
            id: ThreadId::parse(&Self::get::<String>(&row, "id")?).map_err(|e| {
                DbError::QueryFailed {
                    reason: format!("invalid thread id: {e}"),
                }
            })?,
            provider_id: Self::get::<String>(&row, "provider_id")?,
            title: Self::get::<Option<String>>(&row, "title")?,
            token_count: Self::get::<i64>(&row, "token_count")? as u32,
            turn_count: Self::get::<i64>(&row, "turn_count")? as u32,
            created_at: Self::get::<String>(&row, "created_at")?,
            updated_at: Self::get::<String>(&row, "updated_at")?,
        })
    }

    fn map_message_record(row: sqlx::sqlite::SqliteRow) -> Result<MessageRecord, DbError> {
        Ok(MessageRecord {
            id: Some(MessageId::new(Self::get::<i64>(&row, "id")?)),
            thread_id: ThreadId::parse(&Self::get::<String>(&row, "thread_id")?).map_err(|e| {
                DbError::QueryFailed {
                    reason: format!("invalid thread id: {e}"),
                }
            })?,
            seq: Self::get::<i64>(&row, "seq")? as u32,
            role: Self::get::<String>(&row, "role")?,
            content: Self::get::<String>(&row, "content")?,
            tool_call_id: Self::get::<Option<String>>(&row, "tool_call_id")?,
            tool_name: Self::get::<Option<String>>(&row, "tool_name")?,
            tool_calls: Self::get::<Option<String>>(&row, "tool_calls")?,
            created_at: Self::get::<String>(&row, "created_at")?,
        })
    }
}

#[async_trait]
impl ThreadRepository for SqliteThreadRepository {
    async fn upsert_thread(&self, thread: &ThreadRecord) -> Result<(), DbError> {
        sqlx::query(
            r#"
            INSERT INTO threads (id, provider_id, title, token_count, turn_count, created_at, updated_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
            ON CONFLICT(id) DO UPDATE SET
                provider_id = excluded.provider_id,
                title = excluded.title,
                token_count = excluded.token_count,
                turn_count = excluded.turn_count,
                updated_at = excluded.updated_at
            "#,
        )
        .bind(thread.id.to_string())
        .bind(&thread.provider_id)
        .bind(&thread.title)
        .bind(thread.token_count as i64)
        .bind(thread.turn_count as i64)
        .bind(&thread.created_at)
        .bind(&thread.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    async fn get_thread(&self, id: &ThreadId) -> Result<Option<ThreadRecord>, DbError> {
        let row = sqlx::query(
            r#"
            SELECT id, provider_id, title, token_count, turn_count, created_at, updated_at
            FROM threads
            WHERE id = ?1
            "#,
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(Self::map_thread_record).transpose()
    }

    async fn list_threads(&self, limit: u32) -> Result<Vec<ThreadRecord>, DbError> {
        let rows = sqlx::query(
            r#"
            SELECT id, provider_id, title, token_count, turn_count, created_at, updated_at
            FROM threads
            ORDER BY updated_at DESC
            LIMIT ?1
            "#,
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.into_iter().map(Self::map_thread_record).collect()
    }

    async fn delete_thread(&self, id: &ThreadId) -> Result<bool, DbError> {
        let result = sqlx::query(
            r#"
            DELETE FROM threads WHERE id = ?1
            "#,
        )
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(result.rows_affected() > 0)
    }

    async fn add_message(&self, message: &MessageRecord) -> Result<MessageId, DbError> {
        let result = sqlx::query(
            r#"
            INSERT INTO messages (thread_id, seq, role, content, tool_call_id, tool_name, tool_calls, created_at)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
            "#,
        )
        .bind(message.thread_id.to_string())
        .bind(message.seq as i64)
        .bind(&message.role)
        .bind(&message.content)
        .bind(&message.tool_call_id)
        .bind(&message.tool_name)
        .bind(&message.tool_calls)
        .bind(&message.created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(MessageId::new(result.last_insert_rowid()))
    }

    async fn get_messages(&self, thread_id: &ThreadId) -> Result<Vec<MessageRecord>, DbError> {
        let rows = sqlx::query(
            r#"
            SELECT id, thread_id, seq, role, content, tool_call_id, tool_name, tool_calls, created_at
            FROM messages
            WHERE thread_id = ?1
            ORDER BY seq ASC
            "#,
        )
        .bind(thread_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.into_iter().map(Self::map_message_record).collect()
    }

    async fn get_recent_messages(
        &self,
        thread_id: &ThreadId,
        limit: u32,
    ) -> Result<Vec<MessageRecord>, DbError> {
        let rows = sqlx::query(
            r#"
            SELECT id, thread_id, seq, role, content, tool_call_id, tool_name, tool_calls, created_at
            FROM messages
            WHERE thread_id = ?1
            ORDER BY seq DESC
            LIMIT ?2
            "#,
        )
        .bind(thread_id.to_string())
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        let mut messages: Vec<MessageRecord> = rows
            .into_iter()
            .map(Self::map_message_record)
            .collect::<Result<Vec<_>, _>>()?;

        // Reverse to get chronological order
        messages.reverse();
        Ok(messages)
    }

    async fn delete_messages_before(&self, thread_id: &ThreadId, seq: u32) -> Result<u64, DbError> {
        let result = sqlx::query(
            r#"
            DELETE FROM messages
            WHERE thread_id = ?1 AND seq < ?2
            "#,
        )
        .bind(thread_id.to_string())
        .bind(seq as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(result.rows_affected())
    }

    async fn update_thread_stats(
        &self,
        id: &ThreadId,
        token_count: u32,
        turn_count: u32,
    ) -> Result<(), DbError> {
        sqlx::query(
            r#"
            UPDATE threads
            SET token_count = ?1, turn_count = ?2, updated_at = CURRENT_TIMESTAMP
            WHERE id = ?3
            "#,
        )
        .bind(token_count as i64)
        .bind(turn_count as i64)
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }
}
