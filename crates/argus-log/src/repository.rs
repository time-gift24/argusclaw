use async_trait::async_trait;
use sqlx::{SqlitePool, Row};
use argus_protocol::{ThreadId, Result, ArgusError};

use crate::models::TurnLog;

/// Repository trait for turn log persistence.
#[async_trait]
pub trait TurnLogRepository: Send + Sync {
    /// Append a new turn log entry.
    async fn append(&self, log: TurnLog) -> Result<()>;

    /// Get all turn logs for a specific thread.
    async fn get_by_thread(&self, thread_id: &ThreadId) -> Result<Vec<TurnLog>>;

    /// Get active thread IDs ordered by most recent activity (for LRU cleanup).
    async fn get_active_thread_ids(&self, limit: i64) -> Result<Vec<ThreadId>>;

    /// Delete all logs except those for the specified thread IDs.
    async fn delete_except(&self, keep_thread_ids: &[ThreadId]) -> Result<i64>;
}

/// SQLite implementation of TurnLogRepository.
pub struct SqliteTurnLogRepository {
    pool: SqlitePool,
}

impl SqliteTurnLogRepository {
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }
}

#[async_trait]
impl TurnLogRepository for SqliteTurnLogRepository {
    async fn append(&self, log: TurnLog) -> Result<()> {
        sqlx::query(
            r#"
            INSERT INTO turn_logs (thread_id, turn_seq, input_tokens, output_tokens, model, latency_ms, turn_data, created_at)
            VALUES (?, ?, ?, ?, ?, ?, ?, ?)
            "#,
        )
        .bind(log.thread_id.inner().to_string())
        .bind(log.turn_seq)
        .bind(log.input_tokens)
        .bind(log.output_tokens)
        .bind(log.model)
        .bind(log.latency_ms)
        .bind(log.turn_data)
        .bind(log.created_at.to_rfc3339())
        .execute(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

        Ok(())
    }

    async fn get_by_thread(&self, thread_id: &ThreadId) -> Result<Vec<TurnLog>> {
        let rows = sqlx::query(
            r#"
            SELECT thread_id, turn_seq, input_tokens, output_tokens, model, latency_ms, turn_data, created_at
            FROM turn_logs
            WHERE thread_id = ?
            ORDER BY turn_seq ASC
            "#,
        )
        .bind(thread_id.inner().to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

        let logs = rows
            .into_iter()
            .map(|row| {
                let created_at_str: String = row.get("created_at");
                let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now());

                TurnLog {
                    thread_id: ThreadId::parse(&row.get::<String, _>("thread_id")).unwrap_or_default(),
                    turn_seq: row.get("turn_seq"),
                    input_tokens: row.get("input_tokens"),
                    output_tokens: row.get("output_tokens"),
                    model: row.get("model"),
                    latency_ms: row.get("latency_ms"),
                    turn_data: row.get("turn_data"),
                    created_at,
                }
            })
            .collect();

        Ok(logs)
    }

    async fn get_active_thread_ids(&self, limit: i64) -> Result<Vec<ThreadId>> {
        let rows = sqlx::query(
            r#"
            SELECT DISTINCT thread_id
            FROM turn_logs
            ORDER BY created_at DESC
            LIMIT ?
            "#,
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

        let thread_ids = rows
            .into_iter()
            .map(|row| {
                let thread_id: String = row.get("thread_id");
                ThreadId::parse(&thread_id).unwrap_or_default()
            })
            .collect();

        Ok(thread_ids)
    }

    async fn delete_except(&self, keep_thread_ids: &[ThreadId]) -> Result<i64> {
        if keep_thread_ids.is_empty() {
            // Delete all logs if no threads to keep
            let result = sqlx::query("DELETE FROM turn_logs")
                .execute(&self.pool)
                .await
                .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

            return Ok(result.rows_affected() as i64);
        }

        // Build the NOT IN clause dynamically
        let placeholders: Vec<String> = keep_thread_ids.iter().map(|_| "?".to_string()).collect();
        let in_clause = placeholders.join(",");

        let query = format!(
            "DELETE FROM turn_logs WHERE thread_id NOT IN ({})",
            in_clause
        );

        let mut query = sqlx::query(&query);
        for id in keep_thread_ids {
            query = query.bind(id.inner().to_string());
        }

        let result = query
            .execute(&self.pool)
            .await
            .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

        Ok(result.rows_affected() as i64)
    }
}
