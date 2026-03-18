//! ThreadRepository implementation for SQLite.

use async_trait::async_trait;

use crate::error::DbError;
use crate::traits::ThreadRepository;
use crate::types::{MessageId, MessageRecord, ThreadRecord};
use argus_protocol::ThreadId;
use argus_protocol::llm::LlmProviderId;

use super::{ArgusSqlite, DbResult};

#[async_trait]
impl ThreadRepository for ArgusSqlite {
    async fn upsert_thread(&self, thread: &ThreadRecord) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO threads (id, provider_id, title, token_count, turn_count, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                 provider_id = excluded.provider_id,
                 title = excluded.title,
                 token_count = excluded.token_count,
                 turn_count = excluded.turn_count,
                 updated_at = excluded.updated_at",
        )
        .bind(thread.id.to_string())
        .bind(thread.provider_id.into_inner())
        .bind(&thread.title)
        .bind(thread.token_count as i64)
        .bind(thread.turn_count as i64)
        .bind(&thread.created_at)
        .bind(&thread.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(())
    }

    async fn get_thread(&self, id: &ThreadId) -> DbResult<Option<ThreadRecord>> {
        let row = sqlx::query(
            "SELECT id, provider_id, title, token_count, turn_count, created_at, updated_at
             FROM threads WHERE id = ?1",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(|r| self.map_thread_record(r)).transpose()
    }

    async fn list_threads(&self, limit: u32) -> DbResult<Vec<ThreadRecord>> {
        let rows = sqlx::query(
            "SELECT id, provider_id, title, token_count, turn_count, created_at, updated_at
             FROM threads ORDER BY updated_at DESC LIMIT ?1",
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.into_iter()
            .map(|r| self.map_thread_record(r))
            .collect()
    }

    async fn delete_thread(&self, id: &ThreadId) -> DbResult<bool> {
        let result = sqlx::query("DELETE FROM threads WHERE id = ?1")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(result.rows_affected() > 0)
    }

    async fn add_message(&self, message: &MessageRecord) -> DbResult<MessageId> {
        let result = sqlx::query(
            "INSERT INTO messages (thread_id, seq, role, content, tool_call_id, tool_name, tool_calls, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
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
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(MessageId::new(result.last_insert_rowid()))
    }

    async fn get_messages(&self, thread_id: &ThreadId) -> DbResult<Vec<MessageRecord>> {
        let rows = sqlx::query(
            "SELECT id, thread_id, seq, role, content, tool_call_id, tool_name, tool_calls, created_at
             FROM messages WHERE thread_id = ?1 ORDER BY seq ASC",
        )
        .bind(thread_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        rows.into_iter()
            .map(|r| self.map_message_record(r))
            .collect()
    }

    async fn get_recent_messages(
        &self,
        thread_id: &ThreadId,
        limit: u32,
    ) -> DbResult<Vec<MessageRecord>> {
        let rows = sqlx::query(
            "SELECT id, thread_id, seq, role, content, tool_call_id, tool_name, tool_calls, created_at
             FROM messages WHERE thread_id = ?1 ORDER BY seq DESC LIMIT ?2",
        )
        .bind(thread_id.to_string())
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        let mut messages: Vec<MessageRecord> = rows
            .into_iter()
            .map(|r| self.map_message_record(r))
            .collect::<DbResult<Vec<_>>>()?;
        messages.reverse();
        Ok(messages)
    }

    async fn delete_messages_before(&self, thread_id: &ThreadId, seq: u32) -> DbResult<u64> {
        let result = sqlx::query("DELETE FROM messages WHERE thread_id = ?1 AND seq < ?2")
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
    ) -> DbResult<()> {
        sqlx::query(
            "UPDATE threads SET token_count = ?1, turn_count = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?3",
        )
        .bind(token_count as i64)
        .bind(turn_count as i64)
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(())
    }
}

impl ArgusSqlite {
    fn map_thread_record(&self, row: sqlx::sqlite::SqliteRow) -> DbResult<ThreadRecord> {
        Ok(ThreadRecord {
            id: ThreadId::parse(&Self::get_column::<String>(&row, "id")?).map_err(|e| {
                DbError::QueryFailed {
                    reason: format!("invalid thread id: {e}"),
                }
            })?,
            provider_id: LlmProviderId::new(Self::get_column(&row, "provider_id")?),
            title: Self::get_column(&row, "title")?,
            token_count: Self::get_column::<i64>(&row, "token_count")? as u32,
            turn_count: Self::get_column::<i64>(&row, "turn_count")? as u32,
            created_at: Self::get_column(&row, "created_at")?,
            updated_at: Self::get_column(&row, "updated_at")?,
        })
    }

    fn map_message_record(&self, row: sqlx::sqlite::SqliteRow) -> DbResult<MessageRecord> {
        Ok(MessageRecord {
            id: Some(MessageId::new(Self::get_column(&row, "id")?)),
            thread_id: ThreadId::parse(&Self::get_column::<String>(&row, "thread_id")?).map_err(
                |e| DbError::QueryFailed {
                    reason: format!("invalid thread id: {e}"),
                },
            )?,
            seq: Self::get_column::<i64>(&row, "seq")? as u32,
            role: Self::get_column(&row, "role")?,
            content: Self::get_column(&row, "content")?,
            tool_call_id: Self::get_column(&row, "tool_call_id")?,
            tool_name: Self::get_column(&row, "tool_name")?,
            tool_calls: Self::get_column(&row, "tool_calls")?,
            created_at: Self::get_column(&row, "created_at")?,
        })
    }
}
