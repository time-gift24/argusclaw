//! SQLite MCP repository tests.

use async_trait::async_trait;

use crate::error::DbError;
use crate::sqlite::ArgusSqlite;

#[cfg(test)]
mod tests {
    use sqlx::sqlite::SqlitePoolOptions;

    use argus_protocol::{
        AgentId, AgentMcpBinding, AgentMcpServerBinding, AgentRecord, AgentType,
        McpDiscoveredToolRecord, McpServerRecord, McpTransportConfig, McpTransportKind,
        ThinkingConfig,
    };

    use crate::ArgusSqlite;
    use crate::sqlite::migrate;
    use crate::traits::{AgentRepository, McpRepository};

    fn sample_server(display_name: &str) -> McpServerRecord {
        McpServerRecord {
            id: None,
            display_name: display_name.to_string(),
            enabled: true,
            transport: McpTransportConfig::Stdio {
                command: "mcp-server".to_string(),
                args: vec!["--stdio".to_string()],
                env: Default::default(),
            },
            timeout_ms: 30_000,
            status: argus_protocol::mcp::McpServerStatus::Ready,
            last_checked_at: Some("2026-04-01T00:00:00Z".to_string()),
            last_success_at: Some("2026-04-01T00:00:00Z".to_string()),
            last_error: None,
            discovered_tool_count: 0,
        }
    }

    fn sample_tool(
        server_id: i64,
        tool_name_original: &str,
        description: &str,
    ) -> McpDiscoveredToolRecord {
        McpDiscoveredToolRecord {
            server_id,
            tool_name_original: tool_name_original.to_string(),
            description: description.to_string(),
            schema: serde_json::json!({
                "type": "object",
                "properties": {
                    "text": { "type": "string" }
                }
            }),
            annotations: Some(serde_json::json!({ "title": tool_name_original })),
        }
    }

    fn sample_agent(agent_id: i64) -> AgentRecord {
        AgentRecord {
            id: AgentId::new(agent_id),
            display_name: format!("MCP Agent {agent_id}"),
            description: "Agent for MCP tests".to_string(),
            version: "1.0.0".to_string(),
            provider_id: None,
            model_id: None,
            system_prompt: "You are a test agent.".to_string(),
            tool_names: Vec::new(),
            max_tokens: None,
            temperature: None,
            thinking_config: Some(ThinkingConfig::enabled()),
            parent_agent_id: None,
            agent_type: AgentType::Standard,
        }
    }

    #[tokio::test]
    async fn mcp_server_crud_round_trips() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite should connect");
        migrate(&pool).await.expect("migrations should succeed");

        let sqlite = ArgusSqlite::new_with_key_material(pool, vec![7; 32]);
        let record = sample_server("Local Slack");

        let id = McpRepository::upsert_mcp_server(&sqlite, &record)
            .await
            .expect("upsert should succeed");
        let stored = McpRepository::get_mcp_server(&sqlite, id)
            .await
            .expect("get should succeed")
            .expect("stored server should exist");

        assert_eq!(stored.id, Some(id));
        assert_eq!(stored.display_name, "Local Slack");
        assert_eq!(stored.transport.kind(), McpTransportKind::Stdio);
        assert_eq!(
            McpRepository::list_mcp_servers(&sqlite)
                .await
                .unwrap()
                .len(),
            1
        );
        assert!(McpRepository::delete_mcp_server(&sqlite, id).await.unwrap());
        assert!(
            McpRepository::get_mcp_server(&sqlite, id)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn mcp_discovery_snapshots_replace_existing_rows() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite should connect");
        migrate(&pool).await.expect("migrations should succeed");

        let sqlite = ArgusSqlite::new_with_key_material(pool, vec![7; 32]);
        let server_id = McpRepository::upsert_mcp_server(&sqlite, &sample_server("Local Slack"))
            .await
            .expect("server upsert should succeed");

        let first_snapshot = vec![
            sample_tool(server_id, "post_message", "Send a message"),
            sample_tool(server_id, "list_channels", "List channels"),
        ];
        McpRepository::replace_mcp_discovered_tools(&sqlite, server_id, &first_snapshot)
            .await
            .expect("first replace should succeed");

        let stored = McpRepository::list_mcp_discovered_tools(&sqlite, server_id)
            .await
            .expect("list should succeed");
        assert_eq!(stored, first_snapshot);

        let second_snapshot = vec![sample_tool(server_id, "fetch_thread", "Fetch a thread")];
        McpRepository::replace_mcp_discovered_tools(&sqlite, server_id, &second_snapshot)
            .await
            .expect("second replace should succeed");

        let stored = McpRepository::list_mcp_discovered_tools(&sqlite, server_id)
            .await
            .expect("list after replace should succeed");
        assert_eq!(stored, second_snapshot);
    }

    #[tokio::test]
    async fn mcp_agent_bindings_round_trip_and_empty_tool_rows_mean_full_server_access() {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite should connect");
        migrate(&pool).await.expect("migrations should succeed");

        let sqlite = ArgusSqlite::new_with_key_material(pool, vec![7; 32]);
        let slack_server_id = McpRepository::upsert_mcp_server(&sqlite, &sample_server("Slack"))
            .await
            .expect("slack upsert should succeed");
        let github_server_id = McpRepository::upsert_mcp_server(&sqlite, &sample_server("GitHub"))
            .await
            .expect("github upsert should succeed");
        let _ = AgentRepository::upsert(&sqlite, &sample_agent(77))
            .await
            .expect("agent upsert should succeed");

        McpRepository::replace_mcp_discovered_tools(
            &sqlite,
            slack_server_id,
            &[sample_tool(
                slack_server_id,
                "post_message",
                "Send a message",
            )],
        )
        .await
        .expect("tool snapshot should succeed");

        let bindings = vec![
            AgentMcpBinding {
                server: AgentMcpServerBinding {
                    agent_id: AgentId::new(77),
                    server_id: slack_server_id,
                },
                allowed_tools: Some(vec!["post_message".to_string()]),
            },
            AgentMcpBinding {
                server: AgentMcpServerBinding {
                    agent_id: AgentId::new(77),
                    server_id: github_server_id,
                },
                allowed_tools: None,
            },
        ];

        McpRepository::set_agent_mcp_bindings(&sqlite, 77, &bindings)
            .await
            .expect("binding replace should succeed");

        let stored = McpRepository::list_agent_mcp_bindings(&sqlite, 77)
            .await
            .expect("binding list should succeed");
        assert_eq!(stored, bindings);
        assert!(stored.iter().any(|binding| {
            binding.server.server_id == github_server_id && binding.allowed_tools.is_none()
        }));
    }
}

fn status_to_db(status: &argus_protocol::mcp::McpServerStatus) -> &'static str {
    match status {
        argus_protocol::mcp::McpServerStatus::Ready => "ready",
        argus_protocol::mcp::McpServerStatus::Connecting => "connecting",
        argus_protocol::mcp::McpServerStatus::Retrying => "retrying",
        argus_protocol::mcp::McpServerStatus::Failed => "failed",
        argus_protocol::mcp::McpServerStatus::Disabled => "disabled",
    }
}

fn status_from_db(status: &str) -> Result<argus_protocol::mcp::McpServerStatus, DbError> {
    match status {
        "ready" => Ok(argus_protocol::mcp::McpServerStatus::Ready),
        "connecting" => Ok(argus_protocol::mcp::McpServerStatus::Connecting),
        "retrying" => Ok(argus_protocol::mcp::McpServerStatus::Retrying),
        "failed" => Ok(argus_protocol::mcp::McpServerStatus::Failed),
        "disabled" => Ok(argus_protocol::mcp::McpServerStatus::Disabled),
        other => Err(DbError::QueryFailed {
            reason: format!("invalid MCP server status '{other}'"),
        }),
    }
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

fn row_to_server(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<argus_protocol::McpServerRecord, DbError> {
    let transport_json: String = ArgusSqlite::get_column(row, "transport_json")?;
    let transport = decode_json(&transport_json, "mcp server transport")?;
    let status: String = ArgusSqlite::get_column(row, "status")?;

    Ok(argus_protocol::McpServerRecord {
        id: Some(ArgusSqlite::get_column(row, "id")?),
        display_name: ArgusSqlite::get_column(row, "display_name")?,
        enabled: ArgusSqlite::get_column::<i64>(row, "enabled")? != 0,
        transport,
        timeout_ms: ArgusSqlite::get_column::<i64>(row, "timeout_ms")? as u64,
        status: status_from_db(&status)?,
        last_checked_at: ArgusSqlite::get_column(row, "last_checked_at")?,
        last_success_at: ArgusSqlite::get_column(row, "last_success_at")?,
        last_error: ArgusSqlite::get_column(row, "last_error")?,
        discovered_tool_count: ArgusSqlite::get_column::<i64>(row, "discovered_tool_count")? as u32,
    })
}

fn row_to_discovered_tool(
    row: &sqlx::sqlite::SqliteRow,
) -> Result<argus_protocol::McpDiscoveredToolRecord, DbError> {
    let schema_json: String = ArgusSqlite::get_column(row, "schema_json")?;
    let annotations_json: Option<String> = ArgusSqlite::get_column(row, "annotations_json")?;

    Ok(argus_protocol::McpDiscoveredToolRecord {
        server_id: ArgusSqlite::get_column(row, "server_id")?,
        tool_name_original: ArgusSqlite::get_column(row, "tool_name_original")?,
        description: ArgusSqlite::get_column(row, "description")?,
        schema: decode_json(&schema_json, "mcp discovered tool schema")?,
        annotations: match annotations_json {
            Some(value) => Some(decode_json(&value, "mcp discovered tool annotations")?),
            None => None,
        },
    })
}

#[async_trait]
impl crate::traits::McpRepository for ArgusSqlite {
    async fn upsert_mcp_server(
        &self,
        record: &argus_protocol::McpServerRecord,
    ) -> Result<i64, DbError> {
        let transport_json = encode_json(&record.transport, "mcp server transport")?;
        let result = sqlx::query(
            "INSERT INTO mcp_servers (
                id, display_name, enabled, transport_json, timeout_ms, status,
                last_checked_at, last_success_at, last_error, discovered_tool_count, updated_at
             ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, CURRENT_TIMESTAMP)
             ON CONFLICT(id) DO UPDATE SET
                display_name = excluded.display_name,
                enabled = excluded.enabled,
                transport_json = excluded.transport_json,
                timeout_ms = excluded.timeout_ms,
                status = excluded.status,
                last_checked_at = excluded.last_checked_at,
                last_success_at = excluded.last_success_at,
                last_error = excluded.last_error,
                discovered_tool_count = excluded.discovered_tool_count,
                updated_at = CURRENT_TIMESTAMP",
        )
        .bind(record.id)
        .bind(&record.display_name)
        .bind(if record.enabled { 1_i64 } else { 0_i64 })
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

        Ok(record.id.unwrap_or(result.last_insert_rowid()))
    }

    async fn get_mcp_server(
        &self,
        id: i64,
    ) -> Result<Option<argus_protocol::McpServerRecord>, DbError> {
        let row = sqlx::query(
            "SELECT id, display_name, enabled, transport_json, timeout_ms, status,
                    last_checked_at, last_success_at, last_error, discovered_tool_count
             FROM mcp_servers WHERE id = ?",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(|row| row_to_server(&row)).transpose()
    }

    async fn list_mcp_servers(&self) -> Result<Vec<argus_protocol::McpServerRecord>, DbError> {
        let rows = sqlx::query(
            "SELECT id, display_name, enabled, transport_json, timeout_ms, status,
                    last_checked_at, last_success_at, last_error, discovered_tool_count
             FROM mcp_servers
             ORDER BY id",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.iter().map(row_to_server).collect()
    }

    async fn delete_mcp_server(&self, id: i64) -> Result<bool, DbError> {
        let result = sqlx::query("DELETE FROM mcp_servers WHERE id = ?")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        Ok(result.rows_affected() > 0)
    }

    async fn replace_mcp_discovered_tools(
        &self,
        server_id: i64,
        tools: &[argus_protocol::McpDiscoveredToolRecord],
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await.map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        sqlx::query("DELETE FROM mcp_discovered_tools WHERE server_id = ?")
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
                "INSERT INTO mcp_discovered_tools (
                    server_id, tool_name_original, description, schema_json, annotations_json
                 ) VALUES (?, ?, ?, ?, ?)",
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
            "UPDATE mcp_servers
             SET discovered_tool_count = ?, updated_at = CURRENT_TIMESTAMP
             WHERE id = ?",
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

    async fn list_mcp_discovered_tools(
        &self,
        server_id: i64,
    ) -> Result<Vec<argus_protocol::McpDiscoveredToolRecord>, DbError> {
        let rows = sqlx::query(
            "SELECT server_id, tool_name_original, description, schema_json, annotations_json
             FROM mcp_discovered_tools
             WHERE server_id = ?
             ORDER BY rowid",
        )
        .bind(server_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        rows.iter().map(row_to_discovered_tool).collect()
    }

    async fn set_agent_mcp_bindings(
        &self,
        agent_id: i64,
        bindings: &[argus_protocol::AgentMcpBinding],
    ) -> Result<(), DbError> {
        let mut tx = self.pool.begin().await.map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        sqlx::query("DELETE FROM agent_mcp_tools WHERE agent_id = ?")
            .bind(agent_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        sqlx::query("DELETE FROM agent_mcp_servers WHERE agent_id = ?")
            .bind(agent_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        for binding in bindings {
            sqlx::query("INSERT INTO agent_mcp_servers (agent_id, server_id) VALUES (?, ?)")
                .bind(agent_id)
                .bind(binding.server.server_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?;

            if let Some(allowed_tools) = &binding.allowed_tools {
                for tool_name_original in allowed_tools {
                    sqlx::query(
                        "INSERT INTO agent_mcp_tools (agent_id, server_id, tool_name_original)
                         VALUES (?, ?, ?)",
                    )
                    .bind(agent_id)
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
        agent_id: i64,
    ) -> Result<Vec<argus_protocol::AgentMcpBinding>, DbError> {
        let server_rows = sqlx::query(
            "SELECT server_id
             FROM agent_mcp_servers
             WHERE agent_id = ?
             ORDER BY server_id",
        )
        .bind(agent_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        let mut bindings = Vec::with_capacity(server_rows.len());

        for row in server_rows {
            let server_id: i64 = ArgusSqlite::get_column(&row, "server_id")?;
            let tool_rows = sqlx::query(
                "SELECT tool_name_original
                 FROM agent_mcp_tools
                 WHERE agent_id = ? AND server_id = ?
                 ORDER BY tool_name_original",
            )
            .bind(agent_id)
            .bind(server_id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

            let allowed_tools = if tool_rows.is_empty() {
                None
            } else {
                Some(
                    tool_rows
                        .iter()
                        .map(|tool_row| ArgusSqlite::get_column(tool_row, "tool_name_original"))
                        .collect::<Result<Vec<String>, DbError>>()?,
                )
            };

            bindings.push(argus_protocol::AgentMcpBinding {
                server: argus_protocol::AgentMcpServerBinding {
                    agent_id: argus_protocol::AgentId::new(agent_id),
                    server_id,
                },
                allowed_tools,
            });
        }

        Ok(bindings)
    }
}
