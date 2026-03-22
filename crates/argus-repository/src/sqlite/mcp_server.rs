//! McpServerRepository implementation for SQLite.

use std::collections::HashMap;

use async_trait::async_trait;
use serde_json;
use sqlx::Row;

use crate::error::DbError;
use crate::traits::McpServerRepository;
use crate::types::{McpServerId, McpServerRecord};

use super::ArgusSqlite;

#[async_trait]
impl McpServerRepository for ArgusSqlite {
    async fn list(&self) -> Result<Vec<McpServerRecord>, DbError> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, display_name, server_type, command, url, headers, args, auth_token_ciphertext, auth_token_nonce, enabled
            FROM mcp_servers
            ORDER BY display_name ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.into_iter().map(|r| self.map_mcp_record(r)).collect()
    }

    async fn get(&self, id: McpServerId) -> Result<Option<McpServerRecord>, DbError> {
        let row = sqlx::query(
            r#"
            SELECT id, name, display_name, server_type, command, url, headers, args, auth_token_ciphertext, auth_token_nonce, enabled
            FROM mcp_servers
            WHERE id = ?1
            "#,
        )
        .bind(id.into_inner())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(|r| self.map_mcp_record(r)).transpose()
    }

    async fn create(&self, record: &McpServerRecord) -> Result<McpServerId, DbError> {
        let server_type_str = record.server_type.to_string();
        let headers_json = record
            .headers
            .as_ref()
            .map(|h| serde_json::to_string(h).unwrap_or_default());
        let args_json = record
            .args
            .as_ref()
            .map(|a| serde_json::to_string(a).unwrap_or_default());
        let auth_token_blob = record.auth_token_ciphertext.as_deref();
        let auth_nonce_blob = record.auth_token_nonce.as_deref();

        sqlx::query(
            r#"
            INSERT INTO mcp_servers (name, display_name, server_type, command, url, headers, args, auth_token_ciphertext, auth_token_nonce, enabled)
            VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
            "#,
        )
        .bind(&record.name)
        .bind(&record.display_name)
        .bind(&server_type_str)
        .bind(&record.command)
        .bind(&record.url)
        .bind(&headers_json)
        .bind(&args_json)
        .bind(auth_token_blob)
        .bind(auth_nonce_blob)
        .bind(i64::from(record.enabled))
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        let new_id: i64 = sqlx::query_scalar("SELECT last_insert_rowid()")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: format!("failed to get last_insert_rowid: {e}"),
            })?;

        Ok(McpServerId::new(new_id))
    }

    async fn update(&self, record: &McpServerRecord) -> Result<(), DbError> {
        let server_type_str = record.server_type.to_string();
        let headers_json = record
            .headers
            .as_ref()
            .map(|h| serde_json::to_string(h).unwrap_or_default());
        let args_json = record
            .args
            .as_ref()
            .map(|a| serde_json::to_string(a).unwrap_or_default());
        let auth_token_blob = record.auth_token_ciphertext.as_deref();
        let auth_nonce_blob = record.auth_token_nonce.as_deref();

        sqlx::query(
            r#"
            UPDATE mcp_servers
            SET name = ?2, display_name = ?3, server_type = ?4, command = ?5, url = ?6,
                headers = ?7, args = ?8, auth_token_ciphertext = ?9, auth_token_nonce = ?10,
                enabled = ?11, updated_at = CURRENT_TIMESTAMP
            WHERE id = ?1
            "#,
        )
        .bind(record.id.into_inner())
        .bind(&record.name)
        .bind(&record.display_name)
        .bind(&server_type_str)
        .bind(&record.command)
        .bind(&record.url)
        .bind(&headers_json)
        .bind(&args_json)
        .bind(auth_token_blob)
        .bind(auth_nonce_blob)
        .bind(i64::from(record.enabled))
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    async fn delete(&self, id: McpServerId) -> Result<bool, DbError> {
        let result = sqlx::query("DELETE FROM mcp_servers WHERE id = ?1")
            .bind(id.into_inner())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(result.rows_affected() > 0)
    }

    async fn list_enabled(&self) -> Result<Vec<McpServerRecord>, DbError> {
        let rows = sqlx::query(
            r#"
            SELECT id, name, display_name, server_type, command, url, headers, args, auth_token_ciphertext, auth_token_nonce, enabled
            FROM mcp_servers
            WHERE enabled = 1
            ORDER BY display_name ASC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.into_iter().map(|r| self.map_mcp_record(r)).collect()
    }
}

impl ArgusSqlite {
    fn map_mcp_record(&self, row: sqlx::sqlite::SqliteRow) -> Result<McpServerRecord, DbError> {
        use argus_protocol::mcp::ServerType;

        let id: i64 = Self::get_column(&row, "id")?;
        let server_type_str: String = Self::get_column(&row, "server_type")?;
        let server_type: ServerType =
            server_type_str
                .parse()
                .map_err(|e: String| DbError::QueryFailed {
                    reason: format!("invalid server type: {e}"),
                })?;

        // Parse headers JSON if present
        let headers: Option<HashMap<String, String>> = row
            .try_get::<Option<String>, _>("headers")
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?
            .and_then(|json| serde_json::from_str(&json).ok());

        // Parse args JSON if present
        let args: Option<Vec<String>> = row
            .try_get::<Option<String>, _>("args")
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?
            .and_then(|json| serde_json::from_str(&json).ok());

        let enabled: i64 = Self::get_column(&row, "enabled")?;

        Ok(McpServerRecord {
            id: McpServerId::new(id),
            name: Self::get_column(&row, "name")?,
            display_name: Self::get_column(&row, "display_name")?,
            server_type,
            command: Self::get_column(&row, "command")?,
            url: Self::get_column(&row, "url")?,
            headers,
            args,
            auth_token_ciphertext: Self::get_column(&row, "auth_token_ciphertext")?,
            auth_token_nonce: Self::get_column(&row, "auth_token_nonce")?,
            enabled: enabled != 0,
        })
    }
}
