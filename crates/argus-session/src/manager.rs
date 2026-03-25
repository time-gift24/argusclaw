use std::path::PathBuf;
use std::sync::Arc;

use argus_job::JobManager;
use argus_protocol::{
    llm::{ChatMessage, ToolCall},
    AgentId, ArgusError, ProviderId, QueuedUserMessage, Result, SessionId, ThreadControlEvent,
    ThreadEvent, ThreadId,
};
use argus_template::TemplateManager;
use argus_thread::config::ThreadConfigBuilder;
use argus_thread::{CompactorManager, FilePlanStore, ThreadBuilder};
use argus_tool::ToolManager;
use argus_turn::{read_jsonl_events, OnTurnComplete, TraceConfig, TurnConfig, TurnLogEvent};
use dashmap::DashMap;
use sqlx::{Row, SqlitePool};
use tokio::sync::{RwLock, broadcast, mpsc};

use crate::session::{Session, SessionSummary, ThreadSummary};
use argus_protocol::ProviderResolver;

#[derive(Debug)]
struct RecoveredThreadState {
    messages: Vec<ChatMessage>,
    turn_count: u32,
    token_count: u32,
}

/// Manages sessions and their threads.
#[derive(Clone)]
pub struct SessionManager {
    pool: SqlitePool,
    sessions: DashMap<SessionId, Arc<Session>>,
    template_manager: Arc<TemplateManager>,
    provider_resolver: Arc<dyn ProviderResolver>,
    tool_manager: Arc<ToolManager>,
    compactor_manager: Arc<CompactorManager>,
    trace_dir: PathBuf,
    #[allow(dead_code)]
    job_manager: Arc<JobManager>,
}

impl SessionManager {
    /// Create a new SessionManager.
    pub fn new(
        pool: SqlitePool,
        template_manager: Arc<TemplateManager>,
        provider_resolver: Arc<dyn ProviderResolver>,
        tool_manager: Arc<ToolManager>,
        compactor_manager: Arc<CompactorManager>,
        trace_dir: PathBuf,
        job_manager: Arc<JobManager>,
    ) -> Self {
        // Register the dispatch_job tool
        let dispatch_tool = job_manager.clone().create_dispatch_tool();
        tool_manager.register(Arc::new(dispatch_tool));

        // Register the list_subagents tool for querying subagents
        let list_subagents_tool = job_manager.clone().create_list_subagents_tool();
        tool_manager.register(Arc::new(list_subagents_tool));

        Self {
            pool,
            sessions: DashMap::new(),
            template_manager,
            provider_resolver,
            tool_manager,
            compactor_manager,
            trace_dir,
            job_manager,
        }
    }

    /// Broadcast a ThreadEvent to all active sessions.
    pub fn broadcast_event(&self, event: ThreadEvent) {
        for session in self.sessions.iter() {
            session.value().broadcast(event.clone());
        }
    }

    fn spawn_thread_orchestrator(thread: Arc<RwLock<argus_thread::Thread>>) {
        tokio::spawn(async move {
            let (mut control_rx, mailbox) = {
                let mut guard = thread.write().await;
                let control_rx = match guard.take_control_rx() {
                    Some(rx) => rx,
                    None => {
                        tracing::warn!("thread control receiver already taken");
                        return;
                    }
                };
                (control_rx, guard.mailbox())
            };

            let (turn_done_tx, mut turn_done_rx) = mpsc::unbounded_channel();

            loop {
                tokio::select! {
                    Some(control_event) = control_rx.recv() => {
                        let thread_running = {
                            let guard = thread.read().await;
                            guard.is_turn_running()
                        };

                        if thread_running {
                            mailbox.lock().await.push(control_event);
                            continue;
                        }

                        match control_event {
                            ThreadControlEvent::UserMessage { content, msg_override } => {
                                if let Err(error) = Self::start_turn_task(
                                    Arc::clone(&thread),
                                    content,
                                    msg_override,
                                    turn_done_tx.clone(),
                                ).await {
                                    tracing::error!("failed to start queued user turn: {}", error);
                                }
                            }
                            ThreadControlEvent::JobResult(result) => {
                                if let Err(error) = Self::start_turn_task(
                                    Arc::clone(&thread),
                                    result.into_message_text(),
                                    None,
                                    turn_done_tx.clone(),
                                ).await {
                                    tracing::error!("failed to start job-result turn: {}", error);
                                }
                            }
                            ThreadControlEvent::UserInterrupt { .. } => {}
                        }
                    }
                    Some(result) = turn_done_rx.recv() => {
                        let finish_result = {
                            let mut guard = thread.write().await;
                            guard.finish_turn(result)
                        };

                        if let Err(error) = finish_result {
                            tracing::error!("turn failed: {}", error);
                        }

                        let next_message = {
                            let mut mailbox = mailbox.lock().await;
                            mailbox.take_next_turn_message()
                        };

                        if let Some(QueuedUserMessage { content, msg_override }) = next_message {
                            if let Err(error) = Self::start_turn_task(
                                Arc::clone(&thread),
                                content,
                                msg_override,
                                turn_done_tx.clone(),
                            )
                            .await
                            {
                                tracing::error!("failed to start follow-up turn: {}", error);
                            }
                        }
                    }
                    else => break,
                }
            }
        });
    }

    async fn start_turn_task(
        thread: Arc<RwLock<argus_thread::Thread>>,
        content: String,
        msg_override: Option<argus_protocol::MessageOverride>,
        turn_done_tx: mpsc::UnboundedSender<std::result::Result<argus_turn::TurnOutput, argus_thread::ThreadError>>,
    ) -> std::result::Result<(), argus_thread::ThreadError> {
        let turn = {
            let mut guard = thread.write().await;
            guard.begin_turn(content, msg_override).await?
        };

        tokio::spawn(async move {
            let result = turn.execute().await.map_err(argus_thread::ThreadError::TurnFailed);
            let _ = turn_done_tx.send(result);
        });

        Ok(())
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
                    id: SessionId::parse(&row.get::<String, _>("id"))
                        .unwrap_or_else(|_| SessionId::new()),
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

        // Ensure session trace directory exists (for recovery)
        if let Err(e) = self.ensure_session_dir(session_id).await {
            tracing::warn!(session_id = %session_id, error = %e, "Failed to ensure session directory");
        }

        // Load from DB
        let row = sqlx::query("SELECT id, name FROM sessions WHERE id = ?")
            .bind(session_id.to_string())
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
            None => return Err(ArgusError::SessionNotFound(session_id)),
        };

        // Capture self as Arc for use in callbacks (Clone impl uses Arc fields internally)
        let sm = self.clone();

        // Load threads metadata from DB
        let thread_rows = sqlx::query(
            r#"
            SELECT id, template_id, provider_id, title, token_count, turn_count, created_at, updated_at
            FROM threads WHERE session_id = ?
            "#,
        )
        .bind(session_id.to_string())
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
            let token_count: i64 = thread_row.get("token_count");
            let turn_count: i64 = thread_row.get("turn_count");

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
            let updated_at = {
                let updated_at_str: String = thread_row.get("updated_at");
                chrono::DateTime::parse_from_rfc3339(&updated_at_str)
                    .map(|dt| dt.with_timezone(&chrono::Utc))
                    .unwrap_or_else(|_| chrono::Utc::now())
            };
            let trace_cfg =
                TraceConfig::new(true, self.trace_dir.clone())
                    .with_session_id(session_id)
                    .with_turn_start(
                        Some(agent_record.system_prompt.clone()),
                        Some(provider.model_name().to_string()),
                    );
            let on_turn_complete = {
                let sm = sm.clone();
                Arc::new(move |sid: argus_protocol::SessionId, turn_num: u32| {
                    let sm = sm.clone();
                    tokio::spawn(async move {
                        let _ = sm.update_session_turn(sid, turn_num).await;
                    });
                }) as OnTurnComplete
            };
            let mut turn_config = TurnConfig::new();
            turn_config.trace_config = Some(trace_cfg);
            turn_config.on_turn_complete = Some(on_turn_complete);
            let config = ThreadConfigBuilder::default()
                .turn_config(turn_config)
                .build()
                .expect("ThreadConfigBuilder should not fail with defaults");
            let thread = match ThreadBuilder::new()
                .id(thread_id)
                .session_id(session_id)
                .agent_record(Arc::new(agent_record))
                .title(title)
                .provider(provider)
                .tool_manager(self.tool_manager.clone())
                .compactor(compactor)
                .config(config)
                .build()
            {
                Ok(mut t) => {
                    let turn_count = turn_count.max(0) as u32;
                    let token_count = token_count.max(0) as u32;
                    if let Ok(recovered) = recover_thread_state_from_trace(
                            &self.trace_dir,
                            &session_id,
                            &thread_id,
                            (turn_count > 0).then_some(turn_count),
                        )
                        .await
                    {
                        if recovered.turn_count > 0 {
                            t.hydrate_from_persisted_state(
                                recovered.messages,
                                token_count.max(recovered.token_count),
                                turn_count.max(recovered.turn_count),
                                updated_at,
                            );
                        }
                    }

                    Arc::new(RwLock::new(t))
                }
                Err(e) => {
                    tracing::warn!(
                        thread_id = %thread_id_str,
                        error = %e,
                        "Failed to build Thread, skipping"
                    );
                    continue;
                }
            };

            Self::spawn_thread_orchestrator(Arc::clone(&thread));
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
        let session_id = SessionId::new();
        sqlx::query(
            "INSERT INTO sessions (id, name, created_at, updated_at) VALUES (?, ?, datetime('now'), datetime('now'))",
        )
        .bind(session_id.to_string())
        .bind(&name)
        .execute(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

        // Create session trace directory with meta.json
        if let Err(e) = self.ensure_session_dir(session_id).await {
            tracing::warn!(session_id = %session_id, error = %e, "Failed to create session directory");
        }

        Ok(session_id)
    }

    /// Ensure the session trace directory exists with meta.json.
    /// Idempotent: safe to call multiple times.
    pub async fn ensure_session_dir(&self, session_id: SessionId) -> std::io::Result<()> {
        let session_dir = self.trace_dir.join(session_id.to_string());
        tokio::fs::create_dir_all(&session_dir).await?;
        let meta_path = session_dir.join("meta.json");

        // Only create meta.json if it doesn't exist
        if !meta_path.exists() {
            let meta = serde_json::json!({
                "session_id": session_id.to_string(),
                "current_turn": 0,
            });
            tokio::fs::write(&meta_path, serde_json::to_string_pretty(&meta)?).await?;
        }

        Ok(())
    }

    /// Update the current_turn in meta.json after a turn completes.
    pub async fn update_session_turn(&self, session_id: SessionId, turn_number: u32) -> std::io::Result<()> {
        let meta_path = self.trace_dir.join(session_id.to_string()).join("meta.json");

        let meta = if meta_path.exists() {
            let content = tokio::fs::read_to_string(&meta_path).await?;
            serde_json::from_str::<serde_json::Value>(&content).unwrap_or_else(|_| {
                serde_json::json!({
                    "session_id": session_id.to_string(),
                    "current_turn": 0,
                })
            })
        } else {
            serde_json::json!({
                "session_id": session_id.to_string(),
                "current_turn": 0,
            })
        };

        let updated = serde_json::json!({
            "session_id": meta.get("session_id")
                .and_then(|v| v.as_str())
                .unwrap_or(&session_id.to_string()),
            "current_turn": turn_number,
        });
        tokio::fs::write(&meta_path, serde_json::to_string_pretty(&updated)?).await?;
        Ok(())
    }

    /// Delete a session and all its threads.
    pub async fn delete(&self, session_id: SessionId) -> Result<()> {
        // Delete threads belonging to this session (no CASCADE on session_id FK)
        sqlx::query("DELETE FROM threads WHERE session_id = ?")
            .bind(session_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;

        // Delete the session row
        sqlx::query("DELETE FROM sessions WHERE id = ?")
            .bind(session_id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| ArgusError::DatabaseError {
                reason: e.to_string(),
            })?;

        // Remove from memory if loaded
        self.sessions.remove(&session_id);

        // Clean up session trace directory
        let session_dir = self.trace_dir.join(session_id.to_string());
        if session_dir.exists() {
            if let Err(e) = tokio::fs::remove_dir_all(&session_dir).await {
                tracing::warn!(session_id = %session_id, error = %e, "Failed to remove session trace directory");
            }
        }

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

        // Create plan store with persistence
        let plan_store = FilePlanStore::new(self.trace_dir.clone(), &thread_id.inner().to_string());

        // Create Thread directly
        let trace_cfg = TraceConfig::new(true, self.trace_dir.clone())
            .with_session_id(session_id)
            .with_turn_start(
                Some(agent_record.system_prompt.clone()),
                Some(provider.model_name().to_string()),
            );

        // Wire on_turn_complete callback to update session turn count
        let on_turn_complete = {
            let sm = self.clone();
            Arc::new(move |sid: argus_protocol::SessionId, turn_num: u32| {
                let sm = sm.clone();
                tokio::spawn(async move {
                    let _ = sm.update_session_turn(sid, turn_num).await;
                });
            }) as argus_turn::OnTurnComplete
        };

        let mut turn_config = TurnConfig::new();
        turn_config.trace_config = Some(trace_cfg);
        turn_config.on_turn_complete = Some(on_turn_complete);
        let config = ThreadConfigBuilder::default()
            .turn_config(turn_config)
            .build()
            .expect("ThreadConfigBuilder should not fail with defaults");
        let thread = ThreadBuilder::new()
            .id(thread_id)
            .session_id(session_id)
            .agent_record(Arc::new(agent_record))
            .provider(provider)
            .tool_manager(self.tool_manager.clone())
            .compactor(compactor)
            .plan_store(plan_store)
            .config(config)
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
        .bind(session_id.to_string())
        .bind(template_id.inner())
        .bind(provider_id.inner())
        .execute(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

        // Wrap in Arc<RwLock<>> for safe concurrent read access
        let thread_arc = Arc::new(RwLock::new(thread));

        Self::spawn_thread_orchestrator(Arc::clone(&thread_arc));

        // Add to in-memory session
        session.add_thread(thread_arc);

        Ok(thread_id)
    }

    /// Delete a thread from a session.
    pub async fn delete_thread(&self, session_id: SessionId, thread_id: &ThreadId) -> Result<()> {
        // Delete from DB
        sqlx::query("DELETE FROM threads WHERE id = ? AND session_id = ?")
            .bind(thread_id.inner().to_string())
            .bind(session_id.to_string())
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

    /// Rename a persisted session.
    pub async fn rename_session(&self, session_id: SessionId, name: String) -> Result<()> {
        let result = sqlx::query(
            r#"
            UPDATE sessions
            SET name = ?, updated_at = CURRENT_TIMESTAMP
            WHERE id = ?
            "#,
        )
        .bind(name.trim())
        .bind(session_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError {
            reason: e.to_string(),
        })?;

        if result.rows_affected() == 0 {
            return Err(ArgusError::SessionNotFound(session_id));
        }

        Ok(())
    }

    /// Rename a thread title and keep loaded runtime state in sync.
    pub async fn rename_thread(
        &self,
        session_id: SessionId,
        thread_id: &ThreadId,
        title: String,
    ) -> Result<()> {
        let normalized = title.trim().to_string();
        let persisted_title = (!normalized.is_empty()).then_some(normalized);
        let result = sqlx::query(
            r#"
            UPDATE threads
            SET title = ?, updated_at = CURRENT_TIMESTAMP
            WHERE id = ? AND session_id = ?
            "#,
        )
        .bind(persisted_title.as_deref())
        .bind(thread_id.inner().to_string())
        .bind(session_id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError {
            reason: e.to_string(),
        })?;

        if result.rows_affected() == 0 {
            return Err(ArgusError::ThreadNotFound(thread_id.inner().to_string()));
        }

        if let Some(session) = self.sessions.get(&session_id) {
            if let Some(thread) = session.get_thread(thread_id) {
                let mut thread = thread.write().await;
                thread.set_title(persisted_title);
            }
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
        .bind(session_id.to_string())
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

    /// Send a message to a thread via the unified pipe.
    pub async fn send_message(
        &self,
        session_id: SessionId,
        thread_id: &ThreadId,
        message: String,
    ) -> Result<()> {
        // Load session from DB if not already in memory
        self.load(session_id).await?;

        let session = self
            .sessions
            .get(&session_id)
            .ok_or(ArgusError::SessionNotFound(session_id))?;

        let thread = session
            .get_thread(thread_id)
            .ok_or(ArgusError::ThreadNotFound(thread_id.to_string()))?;

        // send_user_message writes to the broadcast pipe (Sender::send is &self).
        let result = thread
            .read()
            .await
            .send_user_message(message, None)
            .map_err(|e| ArgusError::LlmError {
                reason: e.to_string(),
            });
        result
    }

    /// Get the thread message history, falling back to turn trace recovery when
    /// the in-memory history is empty after reloading a persisted session.
    pub async fn get_thread_messages(
        &self,
        session_id: SessionId,
        thread_id: &ThreadId,
    ) -> Result<Vec<ChatMessage>> {
        let session = self.load(session_id).await?;
        let thread = session
            .get_thread(thread_id)
            .ok_or(ArgusError::ThreadNotFound(thread_id.inner().to_string()))?;

        let thread = thread.read().await;
        if !thread.history().is_empty() || thread.turn_count() == 0 {
            return Ok(thread.history().to_vec());
        }

        let turn_count = thread.turn_count();
        drop(thread);

        recover_messages_from_trace(&self.trace_dir, &session_id, thread_id, turn_count).await
    }

    /// Activate a historical thread so it can continue as a live in-memory thread.
    pub async fn activate_thread(
        &self,
        session_id: SessionId,
        thread_id: &ThreadId,
    ) -> Result<(AgentId, Option<ProviderId>)> {
        let row = sqlx::query(
            r#"
            SELECT template_id, provider_id, token_count, turn_count, updated_at
            FROM threads WHERE session_id = ? AND id = ?
            "#,
        )
        .bind(session_id.to_string())
        .bind(thread_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError {
            reason: e.to_string(),
        })?
        .ok_or_else(|| ArgusError::ThreadNotFound(thread_id.inner().to_string()))?;

        let template_id = AgentId::new(row.get::<i64, _>("template_id"));
        let provider_id = Some(ProviderId::new(row.get::<i64, _>("provider_id")));
        let token_count = row.get::<i64, _>("token_count").max(0) as u32;
        let turn_count = row.get::<i64, _>("turn_count").max(0) as u32;
        let updated_at = {
            let updated_at_str: String = row.get("updated_at");
            chrono::DateTime::parse_from_rfc3339(&updated_at_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now())
        };

        let session = self.load(session_id).await?;
        let thread = session
            .get_thread(thread_id)
            .ok_or_else(|| ArgusError::ThreadNotFound(thread_id.inner().to_string()))?;

        let mut thread = thread.write().await;
        if !thread.history().is_empty() {
            return Ok((template_id, provider_id));
        }

        let recovered = recover_thread_state_from_trace(
            &self.trace_dir,
            &session_id,
            thread_id,
            (turn_count > 0).then_some(turn_count),
        )
        .await?;
        if recovered.turn_count == 0 {
            return Ok((template_id, provider_id));
        }

        thread.hydrate_from_persisted_state(
            recovered.messages,
            token_count.max(recovered.token_count),
            turn_count.max(recovered.turn_count),
            updated_at,
        );

        Ok((template_id, provider_id))
    }

    /// Subscribe to thread events.
    pub async fn subscribe(
        &self,
        session_id: SessionId,
        thread_id: &ThreadId,
    ) -> Option<broadcast::Receiver<ThreadEvent>> {
        let session = self.sessions.get(&session_id)?;
        let thread = session.get_thread(thread_id)?;
        let thread = thread.read().await;
        Some(thread.subscribe())
    }
}

async fn recover_messages_from_trace(
    trace_dir: &std::path::Path,
    session_id: &SessionId,
    thread_id: &ThreadId,
    turn_count: u32,
) -> Result<Vec<ChatMessage>> {
    Ok(recover_thread_state_from_trace(trace_dir, session_id, thread_id, Some(turn_count))
        .await?
        .messages)
}

async fn recover_thread_state_from_trace(
    trace_dir: &std::path::Path,
    session_id: &SessionId,
    thread_id: &ThreadId,
    turn_count_hint: Option<u32>,
) -> Result<RecoveredThreadState> {
    let turns_dir = trace_dir
        .join(session_id.to_string())
        .join(thread_id.to_string())
        .join("turns");
    let turn_numbers = resolve_turn_numbers(&turns_dir, turn_count_hint).await?;
    let mut messages = Vec::new();
    let mut token_count = 0;

    for turn_number in &turn_numbers {
        let path = turns_dir.join(format!("{turn_number}.jsonl"));
        let events = read_jsonl_events(&path).await.map_err(|error| ArgusError::DatabaseError {
            reason: format!("failed to recover turn {turn_number} for thread {thread_id}: {error}"),
        })?;

        for event in events {
            match event {
                TurnLogEvent::UserInput { content, .. } => {
                    if !content.trim().is_empty() {
                        messages.push(ChatMessage::user(content));
                    }
                }
                TurnLogEvent::LlmResponse {
                    content,
                    reasoning_content,
                    tool_calls,
                    ..
                } => {
                    if tool_calls.is_empty() {
                        if !content.trim().is_empty() || !reasoning_content.as_deref().unwrap_or("").trim().is_empty() {
                            messages.push(ChatMessage::assistant_with_reasoning(
                                content,
                                reasoning_content,
                            ));
                        }
                    } else {
                        let parsed_tool_calls = tool_calls
                            .into_iter()
                            .map(|value| {
                                serde_json::from_value::<ToolCall>(value).map_err(|error| ArgusError::DatabaseError {
                                    reason: format!(
                                        "failed to recover turn {turn_number} for thread {thread_id}: invalid tool call payload: {error}"
                                    ),
                                })
                            })
                            .collect::<Result<Vec<_>>>()?;

                        messages.push(ChatMessage::assistant_with_tool_calls_and_reasoning(
                            if content.trim().is_empty() {
                                None
                            } else {
                                Some(content)
                            },
                            parsed_tool_calls,
                            reasoning_content,
                        ));
                    }
                }
                TurnLogEvent::ToolResult {
                    id,
                    name,
                    result,
                    error,
                    ..
                } => {
                    let content = error.unwrap_or(result);
                    messages.push(ChatMessage::tool_result(id, name, content));
                }
                TurnLogEvent::TurnEnd { token_usage, .. } => {
                    token_count = token_usage.total_tokens;
                }
                _ => {}
            }
        }
    }

    Ok(RecoveredThreadState {
        messages,
        turn_count: turn_numbers.len() as u32,
        token_count,
    })
}

async fn resolve_turn_numbers(
    turns_dir: &std::path::Path,
    turn_count_hint: Option<u32>,
) -> Result<Vec<u32>> {
    if let Some(turn_count) = turn_count_hint {
        return Ok((1..=turn_count).collect());
    }

    let mut entries = tokio::fs::read_dir(turns_dir)
        .await
        .map_err(|error| ArgusError::DatabaseError {
            reason: format!("failed to inspect trace turns directory {}: {error}", turns_dir.display()),
        })?;
    let mut turn_numbers = Vec::new();

    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|error| ArgusError::DatabaseError {
            reason: format!("failed to inspect trace turns directory {}: {error}", turns_dir.display()),
        })?
    {
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("jsonl") {
            continue;
        }

        let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) else {
            continue;
        };
        let turn_number = stem.parse::<u32>().map_err(|error| ArgusError::DatabaseError {
            reason: format!(
                "failed to parse turn trace filename {}: {error}",
                path.display()
            ),
        })?;
        turn_numbers.push(turn_number);
    }

    turn_numbers.sort_unstable();
    for (index, turn_number) in turn_numbers.iter().enumerate() {
        let expected = index as u32 + 1;
        if *turn_number != expected {
            return Err(ArgusError::DatabaseError {
                reason: format!("missing turn trace file {expected}; found {turn_number} instead"),
            });
        }
    }

    Ok(turn_numbers)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use argus_protocol::llm::{
        CompletionRequest, CompletionResponse, FinishReason, LlmError, ToolCompletionRequest,
        ToolCompletionResponse,
    };
    use argus_protocol::{
        AgentId, AgentRecord, AgentType, ProviderId, Role, SessionId, ThreadControlEvent,
        ThreadEvent, ThreadId, ThreadJobResult,
    };
    use argus_thread::{KeepRecentCompactor, ThreadBuilder};
    use async_trait::async_trait;
    use rust_decimal::Decimal;
    use tokio::time::{sleep, timeout};

    use super::{recover_messages_from_trace, recover_thread_state_from_trace, Session, SessionManager};

    #[derive(Debug)]
    struct CapturingProvider {
        response: String,
        delay: Duration,
        captured_user_inputs: Arc<Mutex<Vec<String>>>,
    }

    impl CapturingProvider {
        fn new(response: &str, delay: Duration) -> Self {
            Self {
                response: response.to_string(),
                delay,
                captured_user_inputs: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn captured_user_inputs(&self) -> Vec<String> {
            self.captured_user_inputs.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl argus_protocol::LlmProvider for CapturingProvider {
        fn model_name(&self) -> &str {
            "capturing"
        }

        fn cost_per_token(&self) -> (Decimal, Decimal) {
            (Decimal::ZERO, Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> std::result::Result<CompletionResponse, LlmError> {
            Err(LlmError::RequestFailed {
                provider: "capturing".to_string(),
                reason: "streaming only in tests".to_string(),
            })
        }

        async fn complete_with_tools(
            &self,
            request: ToolCompletionRequest,
        ) -> std::result::Result<ToolCompletionResponse, LlmError> {
            let last_user_input = request
                .messages
                .iter()
                .rev()
                .find(|message| message.role == argus_protocol::Role::User)
                .map(|message| message.content.clone())
                .unwrap_or_default();
            self.captured_user_inputs
                .lock()
                .unwrap()
                .push(last_user_input);

            sleep(self.delay).await;

            Ok(ToolCompletionResponse {
                content: Some(self.response.clone()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: 12,
                output_tokens: 5,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            })
        }
    }

    fn test_agent_record() -> Arc<AgentRecord> {
        Arc::new(AgentRecord {
            id: AgentId::new(1),
            display_name: "Main Agent".to_string(),
            description: "Main orchestration agent".to_string(),
            version: "1.0.0".to_string(),
            provider_id: Some(ProviderId::new(1)),
            system_prompt: "You are a test orchestrator.".to_string(),
            tool_names: vec![],
            max_tokens: None,
            temperature: None,
            thinking_config: None,
            parent_agent_id: None,
            agent_type: AgentType::Standard,
        })
    }

    fn build_test_thread(
        session_id: SessionId,
        provider: Arc<CapturingProvider>,
    ) -> Arc<tokio::sync::RwLock<argus_thread::Thread>> {
        let compactor = Arc::new(KeepRecentCompactor::with_defaults());
        Arc::new(tokio::sync::RwLock::new(
            ThreadBuilder::new()
                .provider(provider)
                .compactor(compactor)
                .agent_record(test_agent_record())
                .session_id(session_id)
                .build()
                .expect("thread should build"),
        ))
    }

    async fn wait_for_idle(
        thread: &Arc<tokio::sync::RwLock<argus_thread::Thread>>,
        expected_count: usize,
    ) {
        let mut rx = {
            let guard = thread.read().await;
            guard.subscribe()
        };
        let mut idle_count = 0usize;
        timeout(Duration::from_secs(5), async {
            loop {
                match rx.recv().await {
                    Ok(ThreadEvent::Idle { .. }) => {
                        idle_count += 1;
                        if idle_count >= expected_count {
                            break;
                        }
                    }
                    Ok(_) => {}
                    Err(_) => {}
                }
            }
        })
        .await
        .expect("thread should emit idle");
    }

    #[tokio::test]
    async fn recover_messages_from_trace_restores_full_turn_history() {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let session_id = SessionId::new();
        let thread_id = ThreadId::new();
        let turns_dir = temp_dir
            .path()
            .join(session_id.to_string())
            .join(thread_id.to_string())
            .join("turns");
        fs::create_dir_all(&turns_dir).expect("turns dir should exist");

        let turn_one = [
            format!(
                r#"{{"v":"1","thread_id":"{}","turn":1,"ts":"2026-03-25T10:00:00Z","type":"user_input","content":"用户问题一","role":"user"}}"#,
                thread_id
            ),
            format!(
                r#"{{"v":"1","thread_id":"{}","turn":1,"ts":"2026-03-25T10:00:01Z","type":"llm_response","content":"让我查一下","reasoning_content":"先分析再调用工具","tool_calls":[{{"id":"call_1","name":"bash","arguments":{{"cmd":"pwd"}}}}],"finish_reason":"tool_calls"}}"#,
                thread_id
            ),
            format!(
                r#"{{"v":"1","thread_id":"{}","turn":1,"ts":"2026-03-25T10:00:02Z","type":"tool_result","id":"call_1","name":"bash","result":"'/tmp'","duration_ms":12,"error":null}}"#,
                thread_id
            ),
            format!(
                r#"{{"v":"1","thread_id":"{}","turn":1,"ts":"2026-03-25T10:00:03Z","type":"llm_response","content":"总结一","reasoning_content":"推理一","tool_calls":[],"finish_reason":"stop"}}"#,
                thread_id
            ),
        ]
        .join("\n")
            + "\n";
        let turn_two = [
            format!(
                r#"{{"v":"1","thread_id":"{}","turn":2,"ts":"2026-03-25T10:01:00Z","type":"user_input","content":"用户问题二","role":"user"}}"#,
                thread_id
            ),
            format!(
                r#"{{"v":"1","thread_id":"{}","turn":2,"ts":"2026-03-25T10:01:01Z","type":"llm_response","content":"总结二","reasoning_content":"推理二","tool_calls":[],"finish_reason":"stop"}}"#,
                thread_id
            ),
        ]
        .join("\n")
            + "\n";

        fs::write(turns_dir.join("1.jsonl"), turn_one).expect("turn one should write");
        fs::write(turns_dir.join("2.jsonl"), turn_two).expect("turn two should write");

        let messages = recover_messages_from_trace(
            temp_dir.path(),
            &session_id,
            &thread_id,
            2,
        )
        .await
        .expect("trace recovery should succeed");

        assert_eq!(messages.len(), 6);
        assert_eq!(messages[0].role, Role::User);
        assert_eq!(messages[0].content, "用户问题一");
        assert_eq!(messages[1].role, Role::Assistant);
        assert_eq!(messages[1].content, "让我查一下");
        assert_eq!(messages[1].reasoning_content.as_deref(), Some("先分析再调用工具"));
        assert_eq!(messages[1].tool_calls.as_ref().map(Vec::len), Some(1));
        assert_eq!(messages[2].role, Role::Tool);
        assert_eq!(messages[2].tool_call_id.as_deref(), Some("call_1"));
        assert_eq!(messages[2].name.as_deref(), Some("bash"));
        assert_eq!(messages[3].role, Role::Assistant);
        assert_eq!(messages[3].content, "总结一");
        assert_eq!(messages[3].reasoning_content.as_deref(), Some("推理一"));
        assert_eq!(messages[4].role, Role::User);
        assert_eq!(messages[4].content, "用户问题二");
        assert_eq!(messages[5].role, Role::Assistant);
        assert_eq!(messages[5].content, "总结二");
        assert_eq!(messages[5].reasoning_content.as_deref(), Some("推理二"));
    }

    #[tokio::test]
    async fn recover_messages_from_trace_fails_when_turn_file_is_missing() {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let session_id = SessionId::new();
        let thread_id = ThreadId::new();
        let turns_dir = temp_dir
            .path()
            .join(session_id.to_string())
            .join(thread_id.to_string())
            .join("turns");
        fs::create_dir_all(&turns_dir).expect("turns dir should exist");

        fs::write(
            turns_dir.join("1.jsonl"),
            [
                format!(
                    r#"{{"v":"1","thread_id":"{}","turn":1,"ts":"2026-03-25T10:00:00Z","type":"user_input","content":"hi","role":"user"}}"#,
                    thread_id
                ),
                format!(
                    r#"{{"v":"1","thread_id":"{}","turn":1,"ts":"2026-03-25T10:00:01Z","type":"llm_response","content":"hello","reasoning_content":null,"tool_calls":[],"finish_reason":"stop"}}"#,
                    thread_id
                ),
            ]
            .join("\n")
                + "\n",
        )
        .expect("turn one should write");

        let error = recover_messages_from_trace(temp_dir.path(), &session_id, &thread_id, 2)
            .await
            .expect_err("missing turn file should fail");

        assert!(error.to_string().contains("failed to recover turn 2"));
    }

    #[tokio::test]
    async fn recover_thread_state_from_trace_infers_counts_from_files_when_db_counts_are_missing() {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let session_id = SessionId::new();
        let thread_id = ThreadId::new();
        let turns_dir = temp_dir
            .path()
            .join(session_id.to_string())
            .join(thread_id.to_string())
            .join("turns");
        fs::create_dir_all(&turns_dir).expect("turns dir should exist");

        fs::write(
            turns_dir.join("1.jsonl"),
            [
                format!(
                    r#"{{"v":"1","thread_id":"{}","turn":1,"ts":"2026-03-25T10:00:00Z","type":"user_input","content":"hi","role":"user"}}"#,
                    thread_id
                ),
                format!(
                    r#"{{"v":"1","thread_id":"{}","turn":1,"ts":"2026-03-25T10:00:01Z","type":"llm_response","content":"hello","reasoning_content":null,"tool_calls":[],"finish_reason":"stop"}}"#,
                    thread_id
                ),
                format!(
                    r#"{{"v":"1","thread_id":"{}","turn":1,"ts":"2026-03-25T10:00:02Z","type":"turn_end","token_usage":{{"input_tokens":10,"output_tokens":5,"total_tokens":15}},"finish_reason":"stop"}}"#,
                    thread_id
                ),
            ]
            .join("\n")
                + "\n",
        )
        .expect("turn one should write");
        fs::write(
            turns_dir.join("2.jsonl"),
            [
                format!(
                    r#"{{"v":"1","thread_id":"{}","turn":2,"ts":"2026-03-25T10:01:00Z","type":"user_input","content":"again","role":"user"}}"#,
                    thread_id
                ),
                format!(
                    r#"{{"v":"1","thread_id":"{}","turn":2,"ts":"2026-03-25T10:01:01Z","type":"llm_response","content":"welcome back","reasoning_content":null,"tool_calls":[],"finish_reason":"stop"}}"#,
                    thread_id
                ),
                format!(
                    r#"{{"v":"1","thread_id":"{}","turn":2,"ts":"2026-03-25T10:01:02Z","type":"turn_end","token_usage":{{"input_tokens":20,"output_tokens":8,"total_tokens":28}},"finish_reason":"stop"}}"#,
                    thread_id
                ),
            ]
            .join("\n")
                + "\n",
        )
        .expect("turn two should write");

        let recovered = recover_thread_state_from_trace(
            temp_dir.path(),
            &session_id,
            &thread_id,
            None,
        )
        .await
        .expect("trace recovery should succeed");

        assert_eq!(recovered.turn_count, 2);
        assert_eq!(recovered.token_count, 28);
        assert_eq!(recovered.messages.len(), 4);
        assert_eq!(recovered.messages[0].content, "hi");
        assert_eq!(recovered.messages[3].content, "welcome back");
    }

    #[tokio::test]
    async fn busy_thread_remains_visible_while_orchestrator_runs_turn() {
        let session_id = SessionId::new();
        let session = Arc::new(Session::new(session_id, "Test".to_string()));
        let provider = Arc::new(CapturingProvider::new(
            "done",
            Duration::from_millis(150),
        ));
        let thread = build_test_thread(session_id, Arc::clone(&provider));
        let thread_id = thread.read().await.id();

        SessionManager::spawn_thread_orchestrator(Arc::clone(&thread));
        session.add_thread(Arc::clone(&thread));

        {
            let guard = thread.read().await;
            guard
                .send_user_message("hello".to_string(), None)
                .expect("message should queue");
        }

        sleep(Duration::from_millis(30)).await;

        let summaries = session.list_threads().await;
        assert_eq!(summaries.len(), 1);
        assert_eq!(summaries[0].id, thread_id);

        wait_for_idle(&thread, 1).await;
    }

    #[tokio::test]
    async fn idle_job_result_triggers_new_turn_with_synthetic_user_message() {
        let session_id = SessionId::new();
        let provider = Arc::new(CapturingProvider::new("job consumed", Duration::from_millis(10)));
        let thread = build_test_thread(session_id, Arc::clone(&provider));

        SessionManager::spawn_thread_orchestrator(Arc::clone(&thread));

        {
            let guard = thread.read().await;
            guard
                .send_control_event(ThreadControlEvent::JobResult(ThreadJobResult {
                    job_id: "job-42".to_string(),
                    success: true,
                    message: "completed successfully".to_string(),
                    token_usage: None,
                    agent_id: AgentId::new(99),
                    agent_display_name: "Researcher".to_string(),
                    agent_description: "Investigates background context".to_string(),
                }))
                .expect("job result should queue");
        }

        wait_for_idle(&thread, 1).await;

        let captured = provider.captured_user_inputs();
        assert_eq!(captured.len(), 1);
        assert!(captured[0].contains("Job: job-42"));
        assert!(captured[0].contains("Subagent: Researcher"));
        assert!(captured[0].contains("Description: Investigates background context"));
        assert!(captured[0].contains("Result: completed successfully"));
    }

    #[tokio::test]
    async fn running_turn_consumes_job_result_after_idle_if_no_next_iteration_happens() {
        let session_id = SessionId::new();
        let provider = Arc::new(CapturingProvider::new(
            "turn complete",
            Duration::from_millis(120),
        ));
        let thread = build_test_thread(session_id, Arc::clone(&provider));

        SessionManager::spawn_thread_orchestrator(Arc::clone(&thread));

        {
            let guard = thread.read().await;
            guard
                .send_user_message("initial request".to_string(), None)
                .expect("message should queue");
        }

        sleep(Duration::from_millis(20)).await;

        {
            let guard = thread.read().await;
            guard
                .send_control_event(ThreadControlEvent::JobResult(ThreadJobResult {
                    job_id: "job-late".to_string(),
                    success: true,
                    message: "late background answer".to_string(),
                    token_usage: None,
                    agent_id: AgentId::new(100),
                    agent_display_name: "Builder".to_string(),
                    agent_description: "Builds follow-up plans".to_string(),
                }))
                .expect("job result should queue");
        }

        wait_for_idle(&thread, 2).await;

        let captured = provider.captured_user_inputs();
        assert_eq!(captured.len(), 2);
        assert_eq!(captured[0], "initial request");
        assert!(captured[1].contains("Job: job-late"));
        assert!(captured[1].contains("Subagent: Builder"));
    }
}
