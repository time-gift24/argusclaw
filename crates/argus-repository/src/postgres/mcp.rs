//! McpRepository implementation for PostgreSQL.
//!
//! Stub implementation -- MCP tables exist in the schema but the server
//! product does not yet exercise them. This module provides the trait
//! impl so ArgusPostgres satisfies all trait bounds.

use async_trait::async_trait;
use sqlx::Row;

use argus_protocol::{
    AgentId, AgentMcpBinding, McpDiscoveredToolRecord, McpServerRecord,
    mcp::McpServerStatus,
};

use crate::error::DbError;
use crate::traits::McpRepository;

use super::{ArgusPostgres, DbResult};

fn status_to_db(status: &McpServerStatus) -> &'static str {
    match status {
        McpServerStatus::Ready => "ready",
        McpServerStatus::Connecting => "connecting",
        McpServerStatus::Retrying => "retrying",
        McpServerStatus::Failed => "failed",
        McpServerStatus::Disabled => "disabled",
    }
}

fn status_from_db(status: &str) -> Result<McpServerStatus, DbError> {
    match status {
        "ready" => Ok(McpServerStatus::Ready),
        "connecting" => Ok(McpServerStatus::Connecting),
        "retrying" => Ok(McpServerStatus::Retrying),
        "failed" => Ok(McpServerStatus::Failed),
        "disabled" => Ok(McpServerStatus::Disabled),
        other => Err(DbError::QueryFailed {
            reason: format!("invalid MCP server status '{other}'"),
        }),
    }
}

fn get_column<T>(row: &sqlx::postgres::PgRow, col: &str) -> DbResult<T>
where
    T: for<'r> sqlx::decode::Decode<'r, sqlx::Postgres> + sqlx::types::Type<sqlx::Postgres>,
{
    row.try_get(col).map_err(|e| DbError::QueryFailed {
        reason: e.to_string(),
    })
}

fn encode_json<T: serde::Serialize>(value: &T, context: &str) -> Result<String, DbError> {
    serde_json::to_string(value).map_err(|e| DbError::QueryFailed {
        reason: format!("failed to encode {context}: {e}"),
    })
}

fn decode_json<T>(json: &str, context: &str) -> Result<T, DbError>
where
    T: serde::de::DeserializeOwned,
{
    serde_json::from_str(json).map_err(|e| DbError::QueryFailed {
        reason: format!("failed to decode {context}: {e}"),
    })
}

#[async_trait]
impl McpRepository for ArgusPostgres {
    async fn upsert_mcp_server(&self, record: &McpServerRecord) -> Result<i64, DbError> {
        let transport_json = encode_json(&record.transport, "mcp server transport")?;
        let result = sqlx::query(
            "INSERT INTO mcp_servers (id, display_name, enabled, transport_json, timeout_ms, status, \
             last_checked_at, last_success_at, last_error, discovered_tool_count, updated_at) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, NOW()) \
             ON CONFLICT (id) DO UPDATE SET \
                display_name = EXCLUDED.display_name, \
                enabled = EXCLUDED.enabled, \
                transport_json = EXCLUDED.transport_json, \
                timeout_ms = EXCLUDED.timeout_ms, \
                status = EXCLUDED.status, \
                last_checked_at = EXCLUDED.last_checked_at, \
                last_success_at = EXCLUDED.last_success_at, \
                last_error = EXCLUDED.last_error, \
                discovered_tool_count = EXCLUDED.discovered_tool_count, \
                updated_at = NOW()",
        )
        .bind(record.id)
        .bind(&record.display_name)
        .bind(record.enabled)
        .bind(transport_json)
        .bind(record.timeout_ms.min(i64::MAX as u64) as i64)
        .bind(status_to_db(&record.status))
        .bind(&record.last_checked_at)
        .bind(&record.last_success_at)
        .bind(&record.last_error)
        .bind(record.discovered_tool_count as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        let server_id = match record.id {
            Some(id) => id,
            None => {
                let id: i64 = sqlx::query_scalar("SELECT LASTVAL()")
                    .fetch_one(&self.pool)
                    .await
                    .map_err(|e| DbError::QueryFailed {
                        reason: format!("failed to get new mcp server id: {e}"),
                    })?;
                id
            }
        };
        Ok(server_id)
    }

    async fn get_mcp_server(&self, id: i64) -> Result<Option<McpServerRecord>, DbError> {
        let row = sqlx::query(
            "SELECT id, display_name, enabled, transport_json, timeout_ms, status, \
                    last_checked_at, last_success_at, last_error, discovered_tool_count \
             FROM mcp_servers WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(|r| row_to_server(&r)).transpose()
    }

    async fn list_mcp_servers(&self) -> Result<Vec<McpServerRecord>, DbError> {
        let rows = sqlx::query(
            "SELECT id, display_name, enabled, transport_json, timeout_ms, status, \
                    last_checked_at, last_success_at, last_error, discovered_tool_count \
             FROM mcp_servers ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.iter().map(|r| row_to_server(r)).collect()
    }

    async fn delete_mcp_server(&self, id: i64) -> Result<bool, DbError> {
        let result = sqlx::query("DELETE FROM mcp_servers WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(result.rows_affected() > 0)
    }

    async fn replace_mcp_server_tools(
        &self,
        server_id: i64,
        tools: &[McpDiscoveredToolRecord],
    ) -> Result<(), DbError> {
        if let Some(mismatched_tool) = tools.iter().find(|tool| tool.server_id != server_id) {
            return Err(DbError::QueryFailed {
                reason: format!(
                    "mcp tool '{}' belongs to server {} but was written under server {}",
                    mismatched_tool.tool_name_original, mismatched_tool.server_id, server_id
                ),
            });
        }

        let mut tx = self.pool.begin().await.map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        sqlx::query("DELETE FROM mcp_server_tools WHERE server_id = $1")
            .bind(server_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        for tool in tools {
            let schema_json = encode_json(&tool.schema, "mcp discovered tool schema")?;
            let annotations_json = tool
                .annotations
                .as_ref()
                .map(|value| encode_json(value, "mcp discovered tool annotations"))
                .transpose()?;

            sqlx::query(
                "INSERT INTO mcp_server_tools \
                 (server_id, tool_name_original, description, schema_json, annotations_json) \
                 VALUES ($1, $2, $3, $4, $5)",
            )
            .bind(server_id)
            .bind(&tool.tool_name_original)
            .bind(&tool.description)
            .bind(schema_json)
            .bind(annotations_json)
            .execute(&mut *tx)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        }

        sqlx::query(
            "UPDATE mcp_servers SET discovered_tool_count = $1, updated_at = NOW() WHERE id = $2",
        )
        .bind(tools.len().min(i64::MAX as usize) as i64)
        .bind(server_id)
        .execute(&mut *tx)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        tx.commit().await.map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    async fn list_mcp_server_tools(
        &self,
        server_id: i64,
    ) -> Result<Vec<McpDiscoveredToolRecord>, DbError> {
        let rows = sqlx::query(
            "SELECT server_id, tool_name_original, description, schema_json, annotations_json \
             FROM mcp_server_tools WHERE server_id = $1 ORDER BY server_id, tool_name_original",
        )
        .bind(server_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.iter().map(|r| row_to_discovered_tool(r)).collect()
    }

    async fn set_agent_mcp_bindings(
        &self,
        agent_id: AgentId,
        bindings: &[AgentMcpBinding],
    ) -> Result<(), DbError> {
        let agent_id_val = agent_id.inner();
        if let Some(mismatched_binding) = bindings
            .iter()
            .find(|binding| binding.server.agent_id.inner() != agent_id_val)
        {
            return Err(DbError::QueryFailed {
                reason: format!(
                    "mcp binding for server {} belongs to agent {} but was written under agent {}",
                    mismatched_binding.server.server_id,
                    mismatched_binding.server.agent_id.inner(),
                    agent_id_val
                ),
            });
        }

        let mut tx = self.pool.begin().await.map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        sqlx::query("DELETE FROM agent_mcp_tools WHERE agent_id = $1")
            .bind(agent_id_val)
            .execute(&mut *tx)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        sqlx::query("DELETE FROM agent_mcp_servers WHERE agent_id = $1")
            .bind(agent_id_val)
            .execute(&mut *tx)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        for binding in bindings {
            sqlx::query(
                "INSERT INTO agent_mcp_servers (agent_id, server_id, use_tool_whitelist) \
                 VALUES ($1, $2, $3)",
            )
            .bind(agent_id_val)
            .bind(binding.server.server_id)
            .bind(binding.allowed_tools.is_some())
            .execute(&mut *tx)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

            if let Some(allowed_tools) = &binding.allowed_tools {
                for tool_name_original in allowed_tools {
                    sqlx::query(
                        "INSERT INTO agent_mcp_tools (agent_id, server_id, tool_name_original) \
                         VALUES ($1, $2, $3)",
                    )
                    .bind(agent_id_val)
                    .bind(binding.server.server_id)
                    .bind(tool_name_original)
                    .execute(&mut *tx)
                    .await
                    .map_err(|e| DbError::QueryFailed {
                        reason: e.to_string(),
                    })?;
                }
            }
        }

        tx.commit().await.map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    async fn list_agent_mcp_bindings(
        &self,
        agent_id: AgentId,
    ) -> Result<Vec<AgentMcpBinding>, DbError> {
        let agent_id_val = agent_id.inner();
        let server_rows = sqlx::query(
            "SELECT server_id, use_tool_whitelist \
             FROM agent_mcp_servers WHERE agent_id = $1 ORDER BY server_id",
        )
        .bind(agent_id_val)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        let mut bindings = Vec::with_capacity(server_rows.len());

        for row in &server_rows {
            let server_id: i64 = get_column(row, "server_id")?;
            let use_tool_whitelist: bool = get_column(row, "use_tool_whitelist")?;

            let tool_rows = sqlx::query(
                "SELECT tool_name_original \
                 FROM agent_mcp_tools WHERE agent_id = $1 AND server_id = $2 \
                 ORDER BY tool_name_original",
            )
            .bind(agent_id_val)
            .bind(server_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

            let allowed_tools = if use_tool_whitelist {
                Some(
                    tool_rows
                        .iter()
                        .map(|tool_row| get_column(tool_row, "tool_name_original"))
                        .collect::<Result<Vec<String>, DbError>>()?,
                )
            } else {
                None
            };

            bindings.push(AgentMcpBinding {
                server: argus_protocol::AgentMcpServerBinding {
                    agent_id: AgentId::new(agent_id_val),
                    server_id,
                },
                allowed_tools,
            });
        }

        Ok(bindings)
    }
}

fn row_to_server(row: &sqlx::postgres::PgRow) -> Result<McpServerRecord, DbError> {
    let transport_json: String = get_column(row, "transport_json")?;
    let transport = decode_json(&transport_json, "mcp server transport")?;
    let status: String = get_column(row, "status")?;

    Ok(McpServerRecord {
        id: Some(get_column(row, "id")?),
        display_name: get_column(row, "display_name")?,
        enabled: get_column::<bool>(row, "enabled")?,
        transport,
        timeout_ms: get_column::<i64>(row, "timeout_ms")? as u64,
        status: status_from_db(&status)?,
        last_checked_at: get_column(row, "last_checked_at")?,
        last_success_at: get_column(row, "last_success_at")?,
        last_error: get_column(row, "last_error")?,
        discovered_tool_count: get_column::<i64>(row, "discovered_tool_count")? as u32,
    })
}

fn row_to_discovered_tool(row: &sqlx::postgres::PgRow) -> Result<McpDiscoveredToolRecord, DbError> {
    let schema_json: String = get_column(row, "schema_json")?;
    let annotations_json: Option<String> = get_column(row, "annotations_json")?;

    Ok(McpDiscoveredToolRecord {
        server_id: get_column(row, "server_id")?,
        tool_name_original: get_column(row, "tool_name_original")?,
        description: get_column(row, "description")?,
        schema: decode_json(&schema_json, "mcp discovered tool schema")?,
        annotations: match annotations_json {
            Some(value) => Some(decode_json(&value, "mcp discovered tool annotations")?),
            None => None,
        },
    })
}
