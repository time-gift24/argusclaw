use std::sync::Arc;

use argus_protocol::{SessionId, ThreadId, AgentId, ProviderId, Result, ArgusError};
use argus_template::TemplateManager;
use dashmap::DashMap;
use sqlx::{SqlitePool, Row};

use crate::session::{Session, SessionSummary, ThreadSummary};

/// Manages sessions and their threads.
pub struct SessionManager {
    pool: SqlitePool,
    sessions: DashMap<SessionId, Arc<Session>>,
    template_manager: Arc<TemplateManager>,
}

impl SessionManager {
    pub fn new(
        pool: SqlitePool,
        template_manager: Arc<TemplateManager>,
    ) -> Self {
        Self {
            pool,
            sessions: DashMap::new(),
            template_manager,
        }
    }

    /// List all sessions (from DB, not in-memory).
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
        let row = sqlx::query(
            "SELECT id, name FROM sessions WHERE id = ?",
        )
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

        // Load threads for this session
        let thread_rows = sqlx::query(
            r#"
            SELECT id, template_id, provider_id, title, token_count, turn_count, created_at, updated_at
            FROM threads WHERE session_id = ?
            "#,
        )
        .bind(session_id.inner())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

        for thread_row in thread_rows {
            let created_at_str: String = thread_row.get("created_at");
            let updated_at_str: String = thread_row.get("updated_at");

            let created_at = chrono::DateTime::parse_from_rfc3339(&created_at_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now());
            let updated_at = chrono::DateTime::parse_from_rfc3339(&updated_at_str)
                .map(|dt| dt.with_timezone(&chrono::Utc))
                .unwrap_or_else(|_| chrono::Utc::now());

            let thread = crate::session::Thread {
                id: ThreadId::new(thread_row.get::<String, _>("id")),
                session_id,
                template_id: AgentId::new(thread_row.get("template_id")),
                provider_id: ProviderId::new(thread_row.get("provider_id")),
                title: thread_row.get("title"),
                token_count: thread_row.get("token_count"),
                turn_count: thread_row.get("turn_count"),
                created_at,
                updated_at,
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

        // Verify template exists
        let _template = self.template_manager
            .get(template_id)
            .await?
            .ok_or(ArgusError::TemplateNotFound(template_id.inner()))?;

        // Generate thread ID (UUID)
        let thread_id = ThreadId::new(uuid::Uuid::new_v4().to_string());

        // Insert into DB
        sqlx::query(
            r#"
            INSERT INTO threads (id, session_id, template_id, provider_id, token_count, turn_count, created_at, updated_at)
            VALUES (?, ?, ?, ?, 0, 0, datetime('now'), datetime('now'))
            "#,
        )
        .bind(thread_id.inner())
        .bind(session_id.inner())
        .bind(template_id.inner())
        .bind(provider_id.inner())
        .execute(&self.pool)
        .await
        .map_err(|e| ArgusError::DatabaseError { reason: e.to_string() })?;

        // Add to in-memory session
        let thread = crate::session::Thread::new(
            thread_id.clone(),
            session_id,
            template_id,
            provider_id,
        );
        session.add_thread(thread);

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
            .bind(thread_id.inner())
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

    /// Get threads for a session.
    pub async fn list_threads(&self, session_id: SessionId) -> Result<Vec<ThreadSummary>> {
        // If session is loaded, return in-memory threads
        if let Some(session) = self.sessions.get(&session_id) {
            return Ok(session.list_threads());
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
                    id: ThreadId::new(row.get::<String, _>("id")),
                    title: row.get("title"),
                    token_count: row.get("token_count"),
                    turn_count: row.get("turn_count"),
                    updated_at,
                }
            })
            .collect();

        Ok(threads)
    }
}
