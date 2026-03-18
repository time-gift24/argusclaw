use std::sync::Arc;

use argus_protocol::{AgentId, ArgusError, ProviderId, Result, SessionId, ThreadEvent, ThreadId};
use argus_template::TemplateManager;
use argus_thread::{CompactorManager, ThreadConfig};
use argus_tool::ToolManager;
use dashmap::DashMap;
use sqlx::{Row, SqlitePool};
use tokio::sync::broadcast;

use crate::provider_resolver::ProviderResolver;
use crate::runtime_thread::RuntimeThread;
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
        .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

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
            .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

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
        .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

        for thread_row in thread_rows {
            let thread_id_str: String = thread_row.get("id");
            let thread_id = ThreadId::parse(&thread_id_str).unwrap_or_default();
            let template_id: i64 = thread_row.get("template_id");
            let provider_id_val: i64 = thread_row.get("provider_id");

            let created_at_str: String = thread_row.get("created_at");
            let updated_at_str: String = thread_row.get("updated_at");

            let _created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now());
            let _updated_at = chrono::DateTime::parse_from_rfc3339(&updated_at_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now());

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

            // Get template for system prompt
            let template = self
                .template_manager
                .get(AgentId::new(template_id))
                .await
                .ok()
                .flatten();

            let system_prompt = template
                .map(|t| t.system_prompt)
                .unwrap_or_default();

            // Get compactor
            let compactor = self.compactor_manager.default_compactor().clone();

            // Build RuntimeThread
            let title: Option<String> = thread_row.get("title");
            let runtime_thread = match RuntimeThread::new(
                thread_id,
                session_id,
                AgentId::new(template_id),
                provider_id,
                title,
                provider,
                self.tool_manager.clone(),
                compactor,
                system_prompt,
                ThreadConfig::default(),
            ) {
                Ok(t) => Arc::new(t),
                Err(e) => {
                    tracing::warn!(
                        thread_id = %thread_id_str,
                        error = %e,
                        "Failed to build RuntimeThread, skipping"
                    );
                    continue;
                }
            };

            session.add_thread(runtime_thread);
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
            .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

        // Remove from memory if loaded
        self.sessions.remove(&session_id);

        Ok(())
    }

    /// Create a new thread in a session.
    pub async fn create_thread(
        &self,
        session_id: SessionId,
        template_id: AgentId,
        provider_id: ProviderId,
    ) -> Result<ThreadId> {
        // Ensure session is loaded
        let session = self.load(session_id).await?;

        // Verify template exists and get system prompt
        let template = self
            .template_manager
            .get(template_id)
            .await?
            .ok_or(ArgusError::TemplateNotFound(template_id.inner()))?;

        // Resolve provider
        let provider = self.provider_resolver.resolve(provider_id).await?;

        // Generate thread ID (UUID)
        let thread_id = ThreadId::new();

        // Get compactor
        let compactor = self.compactor_manager.default_compactor().clone();

        // Create RuntimeThread
        let runtime_thread = RuntimeThread::new(
            thread_id,
            session_id,
            template_id,
            provider_id,
            None,
            provider,
            self.tool_manager.clone(),
            compactor,
            template.system_prompt.clone(),
            ThreadConfig::default(),
        )
        .map_err(|e| ArgusError::ThreadBuildFailed { reason: e.to_string() })?;

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
        session.add_thread(Arc::new(runtime_thread));

        Ok(thread_id)
    }

    /// Delete a thread from a session.
    pub async fn delete_thread(
        &self,
        session_id: SessionId,
        thread_id: &ThreadId,
    ) -> Result<()> {
        // Delete from DB
        sqlx::query("DELETE FROM threads WHERE id = ? AND session_id = ?")
            .bind(thread_id.inner().to_string())
            .bind(session_id.inner())
            .execute(&self.pool)
            .await
            .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

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
        .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

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

        thread.send_message(message).await
            .map_err(|e| ArgusError::LlmError { reason: e.to_string() })
    }

    /// Subscribe to thread events.
    pub async fn subscribe(
        &self,
        session_id: SessionId,
        thread_id: &ThreadId,
    ) -> Option<broadcast::Receiver<ThreadEvent>> {
        let session = self.sessions.get(&session_id)?;
        let thread = session.get_thread(thread_id)?;
        Some(thread.subscribe().await)
    }
}
