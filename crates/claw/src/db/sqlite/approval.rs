//! SQLite implementation of the approval repository.

use async_trait::async_trait;
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

use crate::approval::{ApprovalDecision, ApprovalRequest, ApprovalResponse};
use crate::db::{ApprovalRepository, DbError};
use crate::protocol::RiskLevel;

/// SQLite-backed approval repository.
pub struct SqliteApprovalRepository {
    pool: SqlitePool,
}

impl SqliteApprovalRepository {
    /// Create a new SQLite approval repository.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    fn map_request(row: sqlx::sqlite::SqliteRow) -> Result<ApprovalRequest, DbError> {
        let risk_level_str: String =
            row.try_get("risk_level")
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?;

        let risk_level = match risk_level_str.as_str() {
            "low" => RiskLevel::Low,
            "medium" => RiskLevel::Medium,
            "high" => RiskLevel::High,
            "critical" => RiskLevel::Critical,
            _ => {
                return Err(DbError::QueryFailed {
                    reason: format!("invalid risk level: {}", risk_level_str),
                });
            }
        };

        let requested_at_str: String =
            row.try_get("requested_at")
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?;

        let requested_at = chrono::DateTime::parse_from_rfc3339(&requested_at_str)
            .map(|dt| dt.with_timezone(&chrono::Utc))
            .map_err(|e| DbError::QueryFailed {
                reason: format!("failed to parse requested_at: {}", e),
            })?;

        Ok(ApprovalRequest {
            id: Uuid::parse_str(&row.try_get::<String, _>("id").map_err(|e| {
                DbError::QueryFailed {
                    reason: e.to_string(),
                }
            })?)
            .map_err(|e| DbError::QueryFailed {
                reason: format!("failed to parse uuid: {}", e),
            })?,
            agent_id: row.try_get("agent_id").map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?,
            tool_name: row.try_get("tool_name").map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?,
            description: row
                .try_get("description")
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?,
            action_summary: row
                .try_get("action_summary")
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?,
            risk_level,
            requested_at,
            timeout_secs: row.try_get::<i64, _>("timeout_secs").map_err(|e| {
                DbError::QueryFailed {
                    reason: e.to_string(),
                }
            })? as u64,
        })
    }
}

#[async_trait]
impl ApprovalRepository for SqliteApprovalRepository {
    async fn insert_request(&self, request: &ApprovalRequest) -> Result<(), DbError> {
        let risk_level = match request.risk_level {
            RiskLevel::Low => "low",
            RiskLevel::Medium => "medium",
            RiskLevel::High => "high",
            RiskLevel::Critical => "critical",
        };

        sqlx::query(
            "INSERT INTO approval_requests (id, agent_id, tool_name, description, action_summary, risk_level, requested_at, timeout_secs)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .bind(request.id.to_string())
        .bind(&request.agent_id)
        .bind(&request.tool_name)
        .bind(&request.description)
        .bind(&request.action_summary)
        .bind(risk_level)
        .bind(request.requested_at.to_rfc3339())
        .bind(request.timeout_secs as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    async fn remove_request(&self, id: Uuid) -> Result<Option<ApprovalRequest>, DbError> {
        let mut transaction = self.pool.begin().await.map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        let row = sqlx::query(
            "SELECT id, agent_id, tool_name, description, action_summary, risk_level, requested_at, timeout_secs
             FROM approval_requests
             WHERE id = ?1",
        )
        .bind(id.to_string())
        .fetch_optional(&mut *transaction)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        let Some(row) = row else {
            transaction
                .commit()
                .await
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?;
            return Ok(None);
        };

        let request = Self::map_request(row)?;

        sqlx::query("DELETE FROM approval_requests WHERE id = ?1")
            .bind(id.to_string())
            .execute(&mut *transaction)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        transaction
            .commit()
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(Some(request))
    }

    async fn list_pending(&self) -> Result<Vec<ApprovalRequest>, DbError> {
        let rows = sqlx::query(
            "SELECT id, agent_id, tool_name, description, action_summary, risk_level, requested_at, timeout_secs
             FROM approval_requests
             ORDER BY requested_at ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.into_iter().map(Self::map_request).collect()
    }

    async fn insert_response(&self, response: &ApprovalResponse) -> Result<(), DbError> {
        let decision = match response.decision {
            ApprovalDecision::Approved => "approved",
            ApprovalDecision::Denied => "denied",
            ApprovalDecision::TimedOut => "timed_out",
        };

        sqlx::query(
            "INSERT INTO approval_responses (request_id, decision, decided_at, decided_by)
             VALUES (?1, ?2, ?3, ?4)",
        )
        .bind(response.request_id.to_string())
        .bind(decision)
        .bind(response.decided_at.to_rfc3339())
        .bind(&response.decided_by)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    async fn clear_pending(&self) -> Result<usize, DbError> {
        let result = sqlx::query("DELETE FROM approval_requests")
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(result.rows_affected() as usize)
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
    async fn insert_and_list_pending() {
        let pool = create_test_pool().await;
        let repo = SqliteApprovalRepository::new(pool);

        let request = ApprovalRequest::new(
            "test-agent".to_string(),
            "shell_exec".to_string(),
            "rm -rf /tmp/test".to_string(),
            60,
            RiskLevel::Critical,
        );

        repo.insert_request(&request).await.unwrap();

        let pending = repo.list_pending().await.unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, request.id);
        assert_eq!(pending[0].tool_name, "shell_exec");
    }

    #[tokio::test]
    async fn remove_request() {
        let pool = create_test_pool().await;
        let repo = SqliteApprovalRepository::new(pool);

        let request = ApprovalRequest::new(
            "test-agent".to_string(),
            "file_write".to_string(),
            "write test".to_string(),
            60,
            RiskLevel::High,
        );

        repo.insert_request(&request).await.unwrap();

        let removed = repo.remove_request(request.id).await.unwrap();
        assert!(removed.is_some());
        assert_eq!(removed.unwrap().id, request.id);

        let pending = repo.list_pending().await.unwrap();
        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn remove_nonexistent_returns_none() {
        let pool = create_test_pool().await;
        let repo = SqliteApprovalRepository::new(pool);

        let result = repo.remove_request(Uuid::new_v4()).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn insert_response() {
        let pool = create_test_pool().await;
        let repo = SqliteApprovalRepository::new(pool);

        let request = ApprovalRequest::new(
            "test-agent".to_string(),
            "shell_exec".to_string(),
            "test".to_string(),
            60,
            RiskLevel::Critical,
        );

        repo.insert_request(&request).await.unwrap();

        let response = ApprovalResponse {
            request_id: request.id,
            decision: ApprovalDecision::Approved,
            decided_at: chrono::Utc::now(),
            decided_by: Some("test-user".to_string()),
        };

        repo.insert_response(&response).await.unwrap();
    }

    #[tokio::test]
    async fn clear_pending() {
        let pool = create_test_pool().await;
        let repo = SqliteApprovalRepository::new(pool);

        for i in 0..3 {
            let request = ApprovalRequest::new(
                "test-agent".to_string(),
                format!("tool_{}", i),
                format!("action {}", i),
                60,
                RiskLevel::Medium,
            );
            repo.insert_request(&request).await.unwrap();
        }

        let count = repo.clear_pending().await.unwrap();
        assert_eq!(count, 3);

        let pending = repo.list_pending().await.unwrap();
        assert!(pending.is_empty());
    }
}
