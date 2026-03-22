//! SQLite implementation for MCP server repository.

use std::collections::HashMap;

use async_trait::async_trait;

use argus_protocol::McpServerConfig;
use argus_protocol::mcp::ServerType;

use crate::error::DbError;
use crate::traits::McpServerRepository;

use super::{ArgusSqlite, DbResult};

#[async_trait]
impl McpServerRepository for ArgusSqlite {
    async fn upsert(&self, config: &McpServerConfig) -> DbResult<()> {
        // Serialize headers to JSON
        let headers_json =
            serde_json::to_string(&config.headers).map_err(|e| DbError::QueryFailed {
                reason: format!("failed to serialize headers: {e}"),
            })?;

        // Serialize args to JSON array
        let args_json = serde_json::to_string(&config.args).map_err(|e| DbError::QueryFailed {
            reason: format!("failed to serialize args: {e}"),
        })?;

        let server_type_str = config.server_type.to_string();
        let enabled_int = if config.enabled { 1 } else { 0 };

        if config.id == 0 {
            sqlx::query(
                "INSERT INTO mcp_servers (name, display_name, server_type, url, headers, command, args, enabled)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )
            .bind(&config.name)
            .bind(&config.display_name)
            .bind(&server_type_str)
            .bind(&config.url)
            .bind(&headers_json)
            .bind(&config.command)
            .bind(&args_json)
            .bind(enabled_int)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        } else {
            sqlx::query(
                "INSERT INTO mcp_servers (id, name, display_name, server_type, url, headers, command, args, enabled)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO UPDATE SET
                     name = excluded.name,
                     display_name = excluded.display_name,
                     server_type = excluded.server_type,
                     url = excluded.url,
                     headers = excluded.headers,
                     command = excluded.command,
                     args = excluded.args,
                     enabled = excluded.enabled,
                     updated_at = CURRENT_TIMESTAMP",
            )
            .bind(config.id)
            .bind(&config.name)
            .bind(&config.display_name)
            .bind(&server_type_str)
            .bind(&config.url)
            .bind(&headers_json)
            .bind(&config.command)
            .bind(&args_json)
            .bind(enabled_int)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        }

        Ok(())
    }

    async fn get(&self, id: i64) -> DbResult<Option<McpServerConfig>> {
        let row = sqlx::query(
            "SELECT id, name, display_name, server_type, url, headers, command, args, enabled
             FROM mcp_servers WHERE id = ?1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(|r| self.map_mcp_server_record(r)).transpose()
    }

    async fn get_by_name(&self, name: &str) -> DbResult<Option<McpServerConfig>> {
        let row = sqlx::query(
            "SELECT id, name, display_name, server_type, url, headers, command, args, enabled
             FROM mcp_servers WHERE name = ?1 LIMIT 1",
        )
        .bind(name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(|r| self.map_mcp_server_record(r)).transpose()
    }

    async fn list(&self) -> DbResult<Vec<McpServerConfig>> {
        let rows = sqlx::query(
            "SELECT id, name, display_name, server_type, url, headers, command, args, enabled
             FROM mcp_servers ORDER BY display_name ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.into_iter()
            .map(|r| self.map_mcp_server_record(r))
            .collect()
    }

    async fn delete(&self, id: i64) -> DbResult<bool> {
        let result = sqlx::query("DELETE FROM mcp_servers WHERE id = ?1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(result.rows_affected() > 0)
    }
}

impl ArgusSqlite {
    fn map_mcp_server_record(&self, row: sqlx::sqlite::SqliteRow) -> DbResult<McpServerConfig> {
        let server_type_str: String = Self::get_column(&row, "server_type")?;
        let server_type: ServerType = server_type_str
            .parse()
            .map_err(|e: String| DbError::QueryFailed { reason: e })?;

        let headers_str: String = Self::get_column(&row, "headers")?;
        let headers: Option<HashMap<String, String>> =
            serde_json::from_str(&headers_str).map_err(|e| DbError::QueryFailed {
                reason: format!("failed to parse headers: {e}"),
            })?;

        let args_str: String = Self::get_column(&row, "args")?;
        let args: Option<Vec<String>> =
            serde_json::from_str(&args_str).map_err(|e| DbError::QueryFailed {
                reason: format!("failed to parse args: {e}"),
            })?;

        let enabled_int: i64 = Self::get_column(&row, "enabled")?;

        Ok(McpServerConfig {
            id: Self::get_column(&row, "id")?,
            name: Self::get_column(&row, "name")?,
            display_name: Self::get_column(&row, "display_name")?,
            server_type,
            url: Self::get_column(&row, "url")?,
            headers,
            command: Self::get_column(&row, "command")?,
            args,
            enabled: enabled_int != 0,
        })
    }
}
