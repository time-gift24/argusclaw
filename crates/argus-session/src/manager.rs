use std::sync::Arc;
use std::time::Instant;

use argus_log::{SqliteTurnLogRepository, TurnLog, TurnLogRepository};
use argus_protocol::TokenUsage;
use argus_protocol::{AgentId, ArgusError, ProviderId, Result, SessionId, ThreadEvent, ThreadId};
use argus_template::TemplateManager;
use argus_thread::{CompactorManager, ThreadBuilder, ThreadConfig};
use argus_tool::ToolManager;
use dashmap::DashMap;
use sqlx::{Row, SqlitePool};
use tokio::sync::broadcast::error::TryRecvError;
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
    turn_log_repository: Arc<dyn TurnLogRepository>,
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
            turn_log_repository: Arc::new(SqliteTurnLogRepository::new(pool.clone())),
            pool,
            sessions: DashMap::new(),
            template_manager,
            provider_resolver,
            tool_manager,
            compactor_manager,
        }
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
            SELECT id, template_id, provider_id, title, token_count, turn_count, created_at, updated_at
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
            let token_count_i64: i64 = thread_row.get("token_count");
            let turn_count_i64: i64 = thread_row.get("turn_count");
            let token_count = u32::try_from(token_count_i64).unwrap_or_default();
            let turn_count = u32::try_from(turn_count_i64).unwrap_or_default();

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

            // Build Thread directly
            let title: Option<String> = thread_row.get("title");
            let thread = match ThreadBuilder::new()
                .id(thread_id)
                .session_id(session_id)
                .agent_record(Arc::new(agent_record))
                .title(title)
                .provider(provider)
                .tool_manager(self.tool_manager.clone())
                .compactor(compactor)
                .config(ThreadConfig::default())
                .token_count(token_count)
                .turn_count(turn_count)
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
            .agent_record(Arc::new(agent_record))
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
        let mut receiver = thread.subscribe();
        let started_at = Instant::now();
        let history_len_before = thread.history().len();
        let model_name = thread.provider().active_model_name();
        thread
            .send_message(message, None)
            .await
            .map_err(|e| ArgusError::LlmError {
                reason: e.to_string(),
            })?;

        let turn_count = thread.turn_count();
        let token_count = thread.token_count();
        let new_messages = thread
            .history()
            .get(history_len_before..)
            .unwrap_or_default()
            .to_vec();
        let turn_usage = Self::extract_turn_usage(&mut receiver, thread_id, turn_count)?;
        drop(thread);

        self.update_thread_stats(thread_id, token_count, turn_count)
            .await?;

        let turn_data =
            serde_json::to_string(&new_messages).map_err(|e| ArgusError::TurnLogError {
                reason: format!("failed to serialize turn_data: {}", e),
            })?;

        let latency_ms = i64::try_from(started_at.elapsed().as_millis()).unwrap_or(i64::MAX);
        let turn_log = TurnLog {
            thread_id: *thread_id,
            turn_seq: i64::from(turn_count),
            input_tokens: i64::from(turn_usage.input_tokens),
            output_tokens: i64::from(turn_usage.output_tokens),
            model: model_name,
            latency_ms,
            turn_data,
            created_at: chrono::Utc::now(),
        };

        self.turn_log_repository
            .append(turn_log)
            .await
            .map_err(|e| ArgusError::TurnLogError {
                reason: e.to_string(),
            })?;

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

    fn extract_turn_usage(
        receiver: &mut broadcast::Receiver<ThreadEvent>,
        thread_id: &ThreadId,
        turn_count: u32,
    ) -> Result<TokenUsage> {
        let expected_thread_id = thread_id.to_string();
        loop {
            match receiver.try_recv() {
                Ok(ThreadEvent::TurnCompleted {
                    thread_id,
                    turn_number,
                    token_usage,
                }) if thread_id == expected_thread_id && turn_number == turn_count => {
                    return Ok(token_usage);
                }
                Ok(_) => continue,
                Err(TryRecvError::Lagged(skipped)) => {
                    return Err(ArgusError::TurnLogError {
                        reason: format!("missed {skipped} thread event(s) before TurnCompleted"),
                    });
                }
                Err(TryRecvError::Empty) | Err(TryRecvError::Closed) => {
                    return Err(ArgusError::TurnLogError {
                        reason: format!(
                            "missing TurnCompleted event for thread {} turn {}",
                            expected_thread_id, turn_count
                        ),
                    });
                }
            }
        }
    }

    async fn update_thread_stats(
        &self,
        thread_id: &ThreadId,
        token_count: u32,
        turn_count: u32,
    ) -> Result<()> {
        sqlx::query(
            r#"
            UPDATE threads
            SET token_count = ?, turn_count = ?, updated_at = datetime('now')
            WHERE id = ?
            "#,
        )
        .bind(i64::from(token_count))
        .bind(i64::from(turn_count))
        .bind(thread_id.inner().to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| ArgusError::TurnLogError {
            reason: format!("failed to update thread stats: {}", e),
        })?;

        Ok(())
    }
}
