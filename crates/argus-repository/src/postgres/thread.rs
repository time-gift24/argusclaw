//! ThreadRepository implementation for PostgreSQL.

use crate::error::DbError;
use crate::traits::ThreadRepository;
use crate::types::{AgentId, MessageId, MessageRecord, ThreadRecord};
use argus_protocol::{LlmProviderId, SessionId, ThreadId};
use async_trait::async_trait;

use super::user::get_column;
use super::{ArgusPostgres, DbResult};

#[async_trait]
impl ThreadRepository for ArgusPostgres {
    async fn upsert_thread(&self, thread: &ThreadRecord) -> DbResult<()> {
        let session_id_str = thread.session_id.as_ref().map(|s| s.to_string());
        let template_id_i64 = thread.template_id.as_ref().map(|t| t.into_inner());

        sqlx::query(
            "INSERT INTO threads (id, provider_id, title, token_count, turn_count, \
             session_id, template_id, model_override, created_at, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) \
             ON CONFLICT (id) DO UPDATE SET \
                provider_id = EXCLUDED.provider_id, \
                title = EXCLUDED.title, \
                token_count = EXCLUDED.token_count, \
                turn_count = EXCLUDED.turn_count, \
                session_id = EXCLUDED.session_id, \
                template_id = EXCLUDED.template_id, \
                model_override = EXCLUDED.model_override, \
                updated_at = EXCLUDED.updated_at",
        )
        .bind(thread.id.to_string())
        .bind(thread.provider_id.into_inner())
        .bind(&thread.title)
        .bind(thread.token_count as i64)
        .bind(thread.turn_count as i64)
        .bind(&session_id_str)
        .bind(template_id_i64)
        .bind(&thread.model_override)
        .bind(&thread.created_at)
        .bind(&thread.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    async fn get_thread(&self, id: &ThreadId) -> DbResult<Option<ThreadRecord>> {
        let row = sqlx::query(
            "SELECT id, provider_id, title, token_count, turn_count, \
                    session_id, template_id, model_override, created_at, updated_at \
             FROM threads WHERE id = $1",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(|row| map_thread_record(&row)).transpose()
    }

    async fn list_threads(&self, limit: u32) -> DbResult<Vec<ThreadRecord>> {
        let rows = sqlx::query(
            "SELECT id, provider_id, title, token_count, turn_count, \
                    session_id, template_id, model_override, created_at, updated_at \
             FROM threads ORDER BY updated_at DESC LIMIT $1",
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.iter().map(|row| map_thread_record(row)).collect()
    }

    async fn list_threads_in_session(&self, session_id: &SessionId) -> DbResult<Vec<ThreadRecord>> {
        let rows = sqlx::query(
            "SELECT id, provider_id, title, token_count, turn_count, \
                    session_id, template_id, model_override, created_at, updated_at \
             FROM threads WHERE session_id = $1 ORDER BY created_at ASC",
        )
        .bind(session_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.iter().map(|row| map_thread_record(row)).collect()
    }

    async fn delete_thread(&self, id: &ThreadId) -> DbResult<bool> {
        let result = sqlx::query("DELETE FROM threads WHERE id = $1")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        Ok(result.rows_affected() > 0)
    }

    async fn delete_threads_in_session(&self, session_id: &SessionId) -> DbResult<u64> {
        let result = sqlx::query("DELETE FROM threads WHERE session_id = $1")
            .bind(session_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        Ok(result.rows_affected())
    }

    async fn add_message(&self, message: &MessageRecord) -> DbResult<MessageId> {
        let _result = sqlx::query(
            "INSERT INTO messages (thread_id, seq, role, content, tool_call_id, tool_name, tool_calls, created_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8)",
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

        let id: i64 = sqlx::query_scalar("SELECT LASTVAL()")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: format!("failed to get last inserted message id: {e}"),
            })?;

        Ok(MessageId::new(id))
    }

    async fn get_messages(&self, thread_id: &ThreadId) -> DbResult<Vec<MessageRecord>> {
        let rows = sqlx::query(
            "SELECT id, thread_id, seq, role, content, tool_call_id, tool_name, tool_calls, created_at \
             FROM messages WHERE thread_id = $1 ORDER BY seq ASC",
        )
        .bind(thread_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        rows.iter().map(|row| map_message_record(row)).collect()
    }

    async fn get_recent_messages(
        &self,
        thread_id: &ThreadId,
        limit: u32,
    ) -> DbResult<Vec<MessageRecord>> {
        let rows = sqlx::query(
            "SELECT id, thread_id, seq, role, content, tool_call_id, tool_name, tool_calls, created_at \
             FROM messages WHERE thread_id = $1 ORDER BY seq DESC LIMIT $2",
        )
        .bind(thread_id.to_string())
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        let mut messages: Vec<MessageRecord> = rows
            .iter()
            .map(|row| map_message_record(row))
            .collect::<DbResult<Vec<_>>>()?;
        messages.reverse();
        Ok(messages)
    }

    async fn delete_messages_before(&self, thread_id: &ThreadId, seq: u32) -> DbResult<u64> {
        let result = sqlx::query("DELETE FROM messages WHERE thread_id = $1 AND seq < $2")
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
        sqlx::query("UPDATE threads SET token_count = $1, turn_count = $2, updated_at = NOW() WHERE id = $3")
            .bind(token_count as i64)
            .bind(turn_count as i64)
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        Ok(())
    }

    async fn rename_thread(
        &self,
        id: &ThreadId,
        session_id: &SessionId,
        title: Option<&str>,
    ) -> DbResult<bool> {
        let result = sqlx::query(
            "UPDATE threads SET title = $1, updated_at = NOW() WHERE id = $2 AND session_id = $3",
        )
        .bind(title)
        .bind(id.to_string())
        .bind(session_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        Ok(result.rows_affected() > 0)
    }

    async fn update_thread_model(
        &self,
        id: &ThreadId,
        session_id: &SessionId,
        provider_id: LlmProviderId,
        model_override: Option<&str>,
    ) -> DbResult<bool> {
        let result = sqlx::query(
            "UPDATE threads SET provider_id = $1, model_override = $2, updated_at = NOW() WHERE id = $3 AND session_id = $4",
        )
        .bind(provider_id.into_inner())
        .bind(model_override)
        .bind(id.to_string())
        .bind(session_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        Ok(result.rows_affected() > 0)
    }

    async fn get_thread_in_session(
        &self,
        thread_id: &ThreadId,
        session_id: &SessionId,
    ) -> DbResult<Option<ThreadRecord>> {
        let row = sqlx::query(
            "SELECT id, provider_id, title, token_count, turn_count, \
                    session_id, template_id, model_override, created_at, updated_at \
             FROM threads WHERE id = $1 AND session_id = $2",
        )
        .bind(thread_id.to_string())
        .bind(session_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(|row| map_thread_record(&row)).transpose()
    }
}

fn map_thread_record(row: &sqlx::postgres::PgRow) -> DbResult<ThreadRecord> {
    let session_id_str: Option<String> = get_column(&row, "session_id")?;
    let session_id = session_id_str.and_then(|s| SessionId::parse(&s).ok());
    let template_id_i64: Option<i64> = get_column(&row, "template_id")?;
    let template_id = template_id_i64.map(AgentId::new);

    Ok(ThreadRecord {
        id: ThreadId::parse(&get_column::<String>(&row, "id")?).map_err(|e| {
            DbError::QueryFailed {
                reason: format!("invalid thread id: {e}"),
            }
        })?,
        provider_id: LlmProviderId::new(get_column(&row, "provider_id")?),
        title: get_column(&row, "title")?,
        token_count: get_column::<i64>(&row, "token_count")? as u32,
        turn_count: get_column::<i64>(&row, "turn_count")? as u32,
        session_id,
        template_id,
        model_override: get_column(&row, "model_override")?,
        created_at: get_column(&row, "created_at")?,
        updated_at: get_column(&row, "updated_at")?,
    })
}

fn map_message_record(row: &sqlx::postgres::PgRow) -> DbResult<MessageRecord> {
    Ok(MessageRecord {
        id: Some(MessageId::new(get_column(&row, "id")?)),
        thread_id: ThreadId::parse(&get_column::<String>(&row, "thread_id")?).map_err(|e| {
            DbError::QueryFailed {
                reason: format!("invalid thread id: {e}"),
            }
        })?,
        seq: get_column::<i64>(&row, "seq")? as u32,
        role: get_column(&row, "role")?,
        content: get_column(&row, "content")?,
        tool_call_id: get_column(&row, "tool_call_id")?,
        tool_name: get_column(&row, "tool_name")?,
        tool_calls: get_column(&row, "tool_calls")?,
        created_at: get_column(&row, "created_at")?,
    })
}
