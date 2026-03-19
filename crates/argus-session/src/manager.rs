use std::sync::Arc;

use argus_protocol::{
    AgentId, ArgusError, CheckpointComparison, CheckpointDetail, CheckpointSummary,
    MessageDiff, ProviderId, Result, SessionId, ThreadEvent, ThreadId, ThreadState, TokenDiff,
};
use argus_template::TemplateManager;
use argus_thread::{CompactorManager, ThreadBuilder, ThreadConfig};
use argus_tool::ToolManager;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde_json;
use sqlx::{Row, SqlitePool};
use tokio::sync::{broadcast, Mutex};

use crate::provider_resolver::ProviderResolver;
use crate::session::{Session, SessionSummary, ThreadSummary};

/// Manages sessions and their threads.
pub struct SessionManager {
    pool: SqlitePool,
    sessions: DashMap<SessionId, Arc<Session>>,
    template_manager: Arc<TemplateManager>,
    provider_resolver: Arc<dyn ProviderResolver>,
    tool_manager: Arc<ToolManager>,
    compactor_manager: Arc<CompactorManager>,
}

impl SessionManager {
    /// Create a new SessionManager.
    pub fn new(
        pool: SqlitePool,
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        tool_manager: Arc<ToolManager>,
        compactor_manager: Arc<CompactorManager>,
    ) -> Self {
        Self {
            pool,
            sessions: DashMap::new(),
            template_manager,
            provider_resolver,
            tool_manager,
            compactor_manager,
        }
    }

    /// Get a reference to the database pool (for testing).
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    /// List all sessions (from DB).
    pub async fn list_sessions(&self) -> Result<Vec<SessionSummary>> {
        let rows = sqlx::query(
            r#"
            SELECT s.id, s.name, s.updated_at, COUNT(t.id) as thread_count
            FROM sessions s
            LEFT JOIN threads t ON t.session_id = s.id
            GROUP BY s.id
            ORDER BY s.updated_at DESC
            "#,
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError {
            reason: e.to_string(),
        })?;

        let sessions = rows
            .into_iter()
            .map(|row| {
                let updated_at_str: String = row.get("updated_at");
                let updated_at = chrono::DateTime::parse_from_rfc3339(&updated_at_str)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now());

                SessionSummary {
                    id: SessionId::new(row.get("id")),
                    name: row.get("name"),
                    thread_count: row.get("thread_count"),
                    updated_at,
                }
            })
            .collect();

        Ok(sessions)
    }

    /// Load a session into memory.
    pub async fn load(&self, session_id: SessionId) -> Result<Arc<Session>> {
        // Check if already loaded
        if let Some(existing) = self.sessions.get(&session_id) {
            return Ok(existing.clone());
        }

        // Load from DB
        let row = sqlx::query("SELECT id, name FROM sessions WHERE id = ?")
            .bind(session_id.inner())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;

        let session = match row {
            Some(row) => {
                let name: String = row.get("name");
                Arc::new(Session::new(session_id, name))
            }
            None => return Err(ArgusError::SessionNotFound(session_id.inner())),
        };

        // Load threads metadata from DB
        let thread_rows = sqlx::query(
            r#"
            SELECT id, template_id, provider_id, title, created_at, updated_at
            FROM threads WHERE session_id = ?
            "#,
        )
        .bind(session_id.inner())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError {
            reason: e.to_string(),
        })?;

        for thread_row in thread_rows {
            let thread_id_str: String = thread_row.get("id");
            let thread_id = ThreadId::parse(&thread_id_str).unwrap_or_default();
            let template_id: i64 = thread_row.get("template_id");
            let provider_id_val: i64 = thread_row.get("provider_id");

            // Resolve provider
            let provider_id = ProviderId::new(provider_id_val);
            let provider = match self.provider_resolver.resolve(provider_id).await {
                Ok(p) => p,
                Err(e) => {
                    tracing::warn!(
                        thread_id = %thread_id_str,
                        provider_id = %provider_id_val,
                        error = %e,
                        "Failed to resolve provider for thread, skipping"
                    );
                    continue;
                }
            };

            // Get agent record (template)
            let agent_record = match self.template_manager.get(AgentId::new(template_id)).await {
                Ok(Some(record)) => record,
                Ok(None) => {
                    tracing::warn!(
                        thread_id = %thread_id_str,
                        template_id = %template_id,
                        "Template not found for thread, skipping"
                    );
                    continue;
                }
                Err(e) => {
                    tracing::warn!(
                        thread_id = %thread_id_str,
                        template_id = %template_id,
                        error = %e,
                        "Failed to get template for thread, skipping"
                    );
                    continue;
                }
            };

            // Get compactor
            let compactor = self.compactor_manager.default_compactor().clone();

            // Load recent messages for this thread (most recent 50)
            let messages = self.load_thread_messages(thread_id).await?;

            // Build Thread directly
            let title: Option<String> = thread_row.get("title");
            let thread = match ThreadBuilder::new()
                .id(thread_id)
                .session_id(session_id)
                .agent_record(agent_record)
                .title(title)
                .provider(provider)
                .tool_manager(self.tool_manager.clone())
                .compactor(compactor)
                .config(ThreadConfig::default())
                .messages(messages)
                .build()
            {
                Ok(t) => Arc::new(Mutex::new(t)),
                Err(e) => {
                    tracing::warn!(
                        thread_id = %thread_id_str,
                        error = %e,
                        "Failed to build Thread, skipping"
                    );
                    continue;
                }
            };

            session.add_thread(thread);
        }

        // Store in memory
        self.sessions.insert(session_id, session.clone());

        Ok(session)
    }

    /// Unload a session from memory.
    pub async fn unload(&self, session_id: SessionId) -> Result<()> {
        self.sessions.remove(&session_id);
        Ok(())
    }

    /// Create a new session.
    pub async fn create(&self, name: String) -> Result<SessionId> {
        let result = sqlx::query(
            "INSERT INTO sessions (name, created_at, updated_at) VALUES (?, datetime('now'), datetime('now'))",
        )
        .bind(&name)
        .execute(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

        let id = result.last_insert_rowid() as i64;
        Ok(SessionId::new(id))
    }

    /// Delete a session and all its threads.
    pub async fn delete(&self, session_id: SessionId) -> Result<()> {
        // Delete from DB (threads will be cascade deleted)
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(session_id.inner())
            .execute(&self.pool)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;

        // Remove from memory if loaded
        self.sessions.remove(&session_id);

        Ok(())
    }

    /// Create a new thread in a session.
    ///
    /// Provider selection logic:
    /// 1. Use `provider_id` if specified
    /// 2. Use `agent_record.provider_id` if set
    /// 3. Use default provider
    pub async fn create_thread(
        &self,
        session_id: SessionId,
        template_id: AgentId,
        explicit_provider_id: Option<ProviderId>,
    ) -> Result<ThreadId> {
        // Ensure session is loaded
        let session = self.load(session_id).await?;

        // Get agent record (template)
        let agent_record = self
            .template_manager
            .get(template_id)
            .await?
            .ok_or(ArgusError::TemplateNotFound(template_id.inner()))?;

        // Resolve provider using priority: explicit > agent_record > default
        let provider_id = explicit_provider_id
            .or(agent_record.provider_id)
            .ok_or(ArgusError::DefaultProviderNotConfigured)?;

        let provider = self.provider_resolver.resolve(provider_id).await?;

        // Generate thread ID (UUID)
        let thread_id = ThreadId::new();

        // Get compactor
        let compactor = self.compactor_manager.default_compactor().clone();

        // Create Thread directly
        let thread = ThreadBuilder::new()
            .id(thread_id)
            .session_id(session_id)
            .agent_record(agent_record)
            .provider(provider)
            .tool_manager(self.tool_manager.clone())
            .compactor(compactor)
            .config(ThreadConfig::default())
            .build()
            .map_err(|e| ArgusError::ThreadBuildFailed {
                reason: e.to_string(),
            })?;

        // Insert into DB
        sqlx::query(
            r#"
            INSERT INTO threads (id, session_id, template_id, provider_id, token_count, turn_count, created_at, updated_at)
            VALUES (?, ?, ?, ?, 0, 0, datetime('now'), datetime('now'))
            "#,
        )
        .bind(thread_id.inner().to_string())
        .bind(session_id.inner())
        .bind(template_id.inner())
        .bind(provider_id.inner())
        .execute(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

        // Add to in-memory session
        session.add_thread(Arc::new(Mutex::new(thread)));

        Ok(thread_id)
    }

    /// Delete a thread from a session.
    pub async fn delete_thread(&self, session_id: SessionId, thread_id: &ThreadId) -> Result<()> {
        // Delete from DB
        sqlx::query("DELETE FROM threads WHERE id = ? AND session_id = ?")
            .bind(thread_id.inner().to_string())
            .bind(session_id.inner())
            .execute(&self.pool)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;

        // Remove from in-memory session if loaded
        if let Some(session) = self.sessions.get(&session_id) {
            session.remove_thread(thread_id);
        }

        Ok(())
    }

    /// Get threads for a session (metadata only, from DB).
    pub async fn list_threads(&self, session_id: SessionId) -> Result<Vec<ThreadSummary>> {
        // If session is loaded, return in-memory threads
        if let Some(session) = self.sessions.get(&session_id) {
            return Ok(session.list_threads().await);
        }

        // Otherwise, load from DB
        let rows = sqlx::query(
            r#"
            SELECT id, title, token_count, turn_count, updated_at
            FROM threads WHERE session_id = ?
            ORDER BY updated_at DESC
            "#,
        )
        .bind(session_id.inner())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError {
            reason: e.to_string(),
        })?;

        let threads = rows
            .into_iter()
            .map(|row| {
                let updated_at_str: String = row.get("updated_at");
                let updated_at = chrono::DateTime::parse_from_rfc3339(&updated_at_str)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now());

                ThreadSummary {
                    id: ThreadId::parse(&row.get::<String, _>("id")).unwrap_or_default(),
                    title: row.get("title"),
                    token_count: row.get("token_count"),
                    turn_count: row.get("turn_count"),
                    updated_at,
                }
            })
            .collect();

        Ok(threads)
    }

    /// Send a message to a thread.
    pub async fn send_message(
        &self,
        session_id: SessionId,
        thread_id: &ThreadId,
        message: String,
    ) -> Result<()> {
        let session = self.load(session_id).await?;
        let thread = session
            .get_thread(thread_id)
            .ok_or(ArgusError::ThreadNotFound(thread_id.inner().to_string()))?;

        let mut thread = thread.lock().await;
        let turn_output = thread
            .send_message(message)
            .await
            .map_err(|e| ArgusError::LlmError {
                reason: e.to_string(),
            })?;

        // TODO: Persist turn_output to database
        // For now, just log it
        tracing::debug!(
            thread_id = %thread_id,
            turn_seq = thread.turn_count(),
            input_tokens = turn_output.token_usage.input_tokens,
            output_tokens = turn_output.token_usage.output_tokens,
            "Turn completed, persistence not yet implemented"
        );

        Ok(())
    }

    /// Subscribe to thread events.
    pub async fn subscribe(
        &self,
        session_id: SessionId,
        thread_id: &ThreadId,
    ) -> Option<broadcast::Receiver<ThreadEvent>> {
        let session = self.sessions.get(&session_id)?;
        let thread = session.get_thread(thread_id)?;
        let thread = thread.lock().await;
        Some(thread.subscribe())
    }

    /// Load recent messages for a thread (most recent 50).
    async fn load_thread_messages(
        &self,
        thread_id: ThreadId,
    ) -> Result<Vec<argus_protocol::llm::ChatMessage>> {
        // Query recent messages from database
        let rows = sqlx::query(
            r#"
            SELECT role, content, tool_call_id, tool_name, tool_calls
            FROM messages
            WHERE thread_id = ?
            ORDER BY seq DESC
            LIMIT 50
            "#,
        )
        .bind(thread_id.inner().to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError {
            reason: e.to_string(),
        })?;

        // Convert rows to ChatMessage (reversing to get correct order)
        let messages: Vec<_> = rows
            .into_iter()
            .rev()
            .map(|row| {
                let role: String = row.get("role");
                let content: String = row.get("content");
                let tool_call_id: Option<String> = row.get("tool_call_id");
                let tool_name: Option<String> = row.get("tool_name");
                let tool_calls: Option<String> = row.get("tool_calls");

                // Parse role
                let role = match role.as_str() {
                    "system" => argus_protocol::llm::Role::System,
                    "user" => argus_protocol::llm::Role::User,
                    "assistant" => argus_protocol::llm::Role::Assistant,
                    "tool" => argus_protocol::llm::Role::Tool,
                    _ => return Err(ArgusError::DatabaseError {
                        reason: format!("Invalid role: {}", role),
                    }),
                };

                // Create ChatMessage based on role
                let message = match role {
                    argus_protocol::llm::Role::System => {
                        argus_protocol::llm::ChatMessage::system(&content)
                    }
                    argus_protocol::llm::Role::User => {
                        argus_protocol::llm::ChatMessage::user(content)
                    }
                    argus_protocol::llm::Role::Assistant => {
                        // Check if there are tool calls
                        if let Some(tool_calls_json) = tool_calls {
                            match serde_json::from_str::<Vec<argus_protocol::llm::ToolCall>>(
                                &tool_calls_json,
                            ) {
                                Ok(tool_calls) => argus_protocol::llm::ChatMessage::assistant_with_tool_calls(
                                    Some(content), tool_calls,
                                ),
                                Err(_) => argus_protocol::llm::ChatMessage::assistant(content),
                            }
                        } else {
                            argus_protocol::llm::ChatMessage::assistant(content)
                        }
                    }
                    argus_protocol::llm::Role::Tool => {
                        let tool_call_id = tool_call_id.ok_or_else(|| ArgusError::DatabaseError {
                            reason: "Tool message missing tool_call_id".to_string(),
                        })?;
                        let tool_name = tool_name.ok_or_else(|| ArgusError::DatabaseError {
                            reason: "Tool message missing tool_name".to_string(),
                        })?;
                        argus_protocol::llm::ChatMessage::tool_result(tool_call_id, tool_name, content)
                    }
                };

                Ok(message)
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(messages)
    }

    /// List all checkpoints for a thread.
    pub async fn list_checkpoints(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Result<Vec<CheckpointSummary>> {
        // Verify thread belongs to session
        self.verify_thread_belongs_to_session(session_id, thread_id)
            .await?;

        let rows = sqlx::query(
            r#"
            SELECT
                turn_seq,
                model,
                input_tokens,
                output_tokens,
                latency_ms,
                created_at,
                messages_count
            FROM turn_logs
            WHERE thread_id = ? AND status = 'completed'
            ORDER BY turn_seq ASC
            "#,
        )
        .bind(thread_id.inner().to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError {
            reason: e.to_string(),
        })?;

        let checkpoints = rows
            .into_iter()
            .map(|row| {
                let created_at_str: String = row.get("created_at");
                let created_at = DateTime::parse_from_rfc3339(&created_at_str)
                    .map(|dt| dt.with_timezone(&Utc))
                    .unwrap_or_else(|_| Utc::now());

                Ok(CheckpointSummary {
                    turn_seq: row.get::<i64, _>("turn_seq") as u32,
                    model: row.get("model"),
                    input_tokens: row.get::<i64, _>("input_tokens") as u32,
                    output_tokens: row.get::<i64, _>("output_tokens") as u32,
                    latency_ms: row.get::<i64, _>("latency_ms") as u64,
                    created_at,
                    message_count: row.get::<i64, _>("messages_count") as u32,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        Ok(checkpoints)
    }

    /// Get detailed information about a specific checkpoint.
    pub async fn get_checkpoint(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
        turn_seq: u32,
    ) -> Result<CheckpointDetail> {
        // Verify thread belongs to session
        self.verify_thread_belongs_to_session(session_id, thread_id)
            .await?;

        let row = sqlx::query(
            r#"
            SELECT
                turn_seq,
                model,
                input_tokens,
                output_tokens,
                latency_ms,
                turn_data,
                created_at
            FROM turn_logs
            WHERE thread_id = ? AND turn_seq = ? AND status = 'completed'
            "#,
        )
        .bind(thread_id.inner().to_string())
        .bind(turn_seq as i64)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError {
            reason: e.to_string(),
        })?
        .ok_or_else(|| ArgusError::CheckpointNotFound {
            thread_id: thread_id.inner().to_string(),
            turn_seq,
        })?;

        let created_at_str: String = row.get("created_at");
        let created_at = DateTime::parse_from_rfc3339(&created_at_str)
            .map(|dt| dt.with_timezone(&Utc))
            .unwrap_or_else(|_| Utc::now());

        // Parse turn_data JSON
        let turn_data_json: String = row.get("turn_data");
        let turn_snapshot: serde_json::Value =
            serde_json::from_str(&turn_data_json).map_err(|e| ArgusError::SerdeError {
                reason: e.to_string(),
            })?;

        // Extract messages from snapshot - simplified version
        let messages = Vec::new(); // TODO: Parse messages from turn_data

        let tool_calls = turn_snapshot["tool_calls"]
            .as_array()
            .map(|arr| serde_json::to_string(arr).unwrap_or_default())
            .unwrap_or_default();

        let llm_response = turn_snapshot["llm_response"].as_object().map(|obj| {
            serde_json::to_string(obj).unwrap_or_default()
        });

        Ok(CheckpointDetail {
            turn_seq: row.get::<i64, _>("turn_seq") as u32,
            model: row.get("model"),
            token_usage: argus_protocol::TokenUsage {
                input_tokens: row.get::<i64, _>("input_tokens") as u32,
                output_tokens: row.get::<i64, _>("output_tokens") as u32,
                total_tokens: (row.get::<i64, _>("input_tokens") as u32
                    + row.get::<i64, _>("output_tokens") as u32),
            },
            latency_ms: row.get::<i64, _>("latency_ms") as u64,
            created_at,
            messages,
            tool_calls,
            llm_response,
        })
    }

    /// Compare two checkpoints.
    pub async fn compare_checkpoints(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
        turn_seq_a: u32,
        turn_seq_b: u32,
    ) -> Result<CheckpointComparison> {
        let turn_a = self
            .get_checkpoint(session_id, thread_id, turn_seq_a)
            .await?;
        let turn_b = self
            .get_checkpoint(session_id, thread_id, turn_seq_b)
            .await?;

        let token_diff = TokenDiff {
            input_delta: turn_b.token_usage.input_tokens as i32
                - turn_a.token_usage.input_tokens as i32,
            output_delta: turn_b.token_usage.output_tokens as i32
                - turn_a.token_usage.output_tokens as i32,
            total_delta: turn_b.token_usage.total_tokens as i32
                - turn_a.token_usage.total_tokens as i32,
        };

        let message_diff = MessageDiff {
            count_a: turn_a.messages.len(),
            count_b: turn_b.messages.len(),
            count_delta: turn_b.messages.len() as isize - turn_a.messages.len() as isize,
        };

        Ok(CheckpointComparison {
            turn_a,
            turn_b,
            token_diff,
            message_diff,
        })
    }

    /// Get message history at a specific turn.
    pub async fn get_history_at_turn(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
        turn_seq: u32,
    ) -> Result<Vec<argus_protocol::llm::ChatMessage>> {
        // Verify thread belongs to session
        self.verify_thread_belongs_to_session(session_id, thread_id)
            .await?;

        let rows = sqlx::query(
            r#"
            SELECT role, content, tool_call_id, tool_name, tool_calls
            FROM messages
            WHERE thread_id = ? AND turn_seq <= ?
            ORDER BY seq ASC
            "#,
        )
        .bind(thread_id.inner().to_string())
        .bind(turn_seq as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError {
            reason: e.to_string(),
        })?;

        self.parse_message_rows(rows)
    }

    /// Get all messages for a thread.
    pub async fn get_thread_messages(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Result<Vec<argus_protocol::llm::ChatMessage>> {
        // Verify thread belongs to session
        self.verify_thread_belongs_to_session(session_id, thread_id)
            .await?;

        let rows = sqlx::query(
            r#"
            SELECT role, content, tool_call_id, tool_name, tool_calls
            FROM messages
            WHERE thread_id = ?
            ORDER BY seq ASC
            "#,
        )
        .bind(thread_id.inner().to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError {
            reason: e.to_string(),
        })?;

        self.parse_message_rows(rows)
    }

    /// Get recent messages for a thread.
    pub async fn get_recent_messages(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
        limit: u32,
    ) -> Result<Vec<argus_protocol::llm::ChatMessage>> {
        // Verify thread belongs to session
        self.verify_thread_belongs_to_session(session_id, thread_id)
            .await?;

        let rows = sqlx::query(
            r#"
            SELECT role, content, tool_call_id, tool_name, tool_calls
            FROM messages
            WHERE thread_id = ?
            ORDER BY seq DESC
            LIMIT ?
            "#,
        )
        .bind(thread_id.inner().to_string())
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError {
            reason: e.to_string(),
        })?;

        // Reverse to get correct order
        let messages = self.parse_message_rows(rows)?;
        Ok(messages.into_iter().rev().collect())
    }

    /// Rollback a thread to a specific turn.
    pub async fn rollback_to_turn(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
        target_turn_seq: u32,
    ) -> Result<ThreadState> {
        // 1. Verify thread belongs to session
        self.verify_thread_belongs_to_session(session_id, thread_id)
            .await?;

        // 2. Find last message seq of target turn
        let last_seq: Option<i64> = sqlx::query_scalar(
            "SELECT MAX(seq) FROM messages WHERE thread_id = ? AND turn_seq <= ?",
        )
        .bind(thread_id.inner().to_string())
        .bind(target_turn_seq as i64)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError {
            reason: e.to_string(),
        })?;

        let last_seq = last_seq.ok_or_else(|| ArgusError::CheckpointNotFound {
            thread_id: thread_id.inner().to_string(),
            turn_seq: target_turn_seq,
        })?;

        // 3. Begin transaction for rollback
        let mut tx = self.pool.begin().await.map_err(|e| ArgusError::DatabaseError {
            reason: e.to_string(),
        })?;

        // Delete messages after target turn
        sqlx::query("DELETE FROM messages WHERE thread_id = ? AND seq > ?")
            .bind(thread_id.inner().to_string())
            .bind(last_seq)
            .execute(&mut *tx)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;

        // Delete turn_logs after target turn
        sqlx::query("DELETE FROM turn_logs WHERE thread_id = ? AND turn_seq > ?")
            .bind(thread_id.inner().to_string())
            .bind(target_turn_seq as i64)
            .execute(&mut *tx)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;

        // Update thread stats
        let (token_count, turn_count): (i64, i64) = sqlx::query_as(
            "SELECT
                COALESCE(SUM(input_tokens + output_tokens), 0) as tokens,
                COUNT(*) as turns
             FROM turn_logs WHERE thread_id = ?",
        )
        .bind(thread_id.inner().to_string())
        .fetch_one(&mut *tx)
        .await
        .map_err(|e| ArgusError::DatabaseError {
            reason: e.to_string(),
        })?;

        sqlx::query(
            "UPDATE threads SET
             token_count = ?,
             turn_count = ?,
             updated_at = datetime('now')
             WHERE id = ?",
        )
        .bind(token_count)
        .bind(turn_count)
        .bind(thread_id.inner().to_string())
        .execute(&mut *tx)
        .await
        .map_err(|e| ArgusError::DatabaseError {
            reason: e.to_string(),
        })?;

        tx.commit().await.map_err(|e| ArgusError::DatabaseError {
            reason: e.to_string(),
        })?;

        // 4. Remove session from cache to force reload on next access
        self.sessions.remove(&session_id);

        // 5. Query thread state from database
        let thread_row = sqlx::query(
            "SELECT title, token_count, turn_count FROM threads WHERE id = ?"
        )
        .bind(thread_id.inner().to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError {
            reason: e.to_string(),
        })?;

        let (title, token_count, turn_count) = match thread_row {
            Some(row) => {
                let title: Option<String> = row.get("title");
                let token_count: i64 = row.get("token_count");
                let turn_count: i64 = row.get("turn_count");
                (title, token_count as u32, turn_count as u32)
            }
            None => return Err(ArgusError::ThreadNotFound(thread_id.inner().to_string())),
        };

        // Count remaining messages
        let message_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM messages WHERE thread_id = ?"
        )
        .bind(thread_id.inner().to_string())
        .fetch_one(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError {
            reason: e.to_string(),
        })?;

        // 6. Return state
        Ok(ThreadState {
            thread_id,
            title,
            message_count: message_count as usize,
            turn_count,
            token_count,
            last_turn_seq: Some(target_turn_seq),
        })
    }

    /// Verify that a thread belongs to a session.
    async fn verify_thread_belongs_to_session(
        &self,
        session_id: SessionId,
        thread_id: ThreadId,
    ) -> Result<()> {
        let exists = sqlx::query(
            "SELECT 1 FROM threads WHERE id = ? AND session_id = ?",
        )
        .bind(thread_id.inner().to_string())
        .bind(session_id.inner())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError {
            reason: e.to_string(),
        })?;

        if exists.is_none() {
            return Err(ArgusError::ThreadNotFoundInSession {
                thread_id: thread_id.inner().to_string(),
                session_id: session_id.inner(),
            });
        }

        Ok(())
    }

    /// Parse message rows into ChatMessage objects.
    fn parse_message_rows(
        &self,
        rows: Vec<sqlx::sqlite::SqliteRow>,
    ) -> Result<Vec<argus_protocol::llm::ChatMessage>> {
        rows.into_iter()
            .map(|row| {
                let role: String = row.get("role");
                let content: String = row.get("content");
                let tool_call_id: Option<String> = row.get("tool_call_id");
                let tool_name: Option<String> = row.get("tool_name");
                let tool_calls: Option<String> = row.get("tool_calls");

                let role = match role.as_str() {
                    "system" => argus_protocol::llm::Role::System,
                    "user" => argus_protocol::llm::Role::User,
                    "assistant" => argus_protocol::llm::Role::Assistant,
                    "tool" => argus_protocol::llm::Role::Tool,
                    _ => {
                        return Err(ArgusError::DatabaseError {
                            reason: format!("Invalid role: {}", role),
                        })
                    }
                };

                let message = match role {
                    argus_protocol::llm::Role::System => {
                        argus_protocol::llm::ChatMessage::system(&content)
                    }
                    argus_protocol::llm::Role::User => {
                        argus_protocol::llm::ChatMessage::user(content)
                    }
                    argus_protocol::llm::Role::Assistant => {
                        if let Some(tool_calls_json) = tool_calls {
                            match serde_json::from_str::<Vec<argus_protocol::llm::ToolCall>>(
                                &tool_calls_json,
                            ) {
                                Ok(tool_calls) => {
                                    argus_protocol::llm::ChatMessage::assistant_with_tool_calls(
                                        Some(content), tool_calls,
                                    )
                                }
                                Err(_) => argus_protocol::llm::ChatMessage::assistant(content),
                            }
                        } else {
                            argus_protocol::llm::ChatMessage::assistant(content)
                        }
                    }
                    argus_protocol::llm::Role::Tool => {
                        let tool_call_id = tool_call_id.ok_or_else(|| ArgusError::DatabaseError {
                            reason: "Tool message missing tool_call_id".to_string(),
                        })?;
                        let tool_name = tool_name.ok_or_else(|| ArgusError::DatabaseError {
                            reason: "Tool message missing tool_name".to_string(),
                        })?;
                        argus_protocol::llm::ChatMessage::tool_result(
                            tool_call_id,
                            tool_name,
                            content,
                        )
                    }
                };

                Ok(message)
            })
            .collect()
    }
}
