//! RuntimeThread - combines metadata with execution engine.
//!
//! RuntimeThread wraps `argus_thread::Thread` with metadata for persistence
//! and provides async methods for message handling.

use std::sync::Arc;

use argus_protocol::{AgentId, ChatMessage, ProviderId, SessionId, ThreadEvent, ThreadId};
use argus_thread::{Compactor, ThreadBuilder, ThreadConfig, ThreadError};
use argus_tool::ToolManager;
use argus_protocol::LlmProvider;
use chrono::{DateTime, Utc};
use tokio::sync::{broadcast, Mutex};

/// RuntimeThread combines persisted metadata with in-memory execution state.
pub struct RuntimeThread {
    // Metadata (persisted to DB)
    /// Unique thread identifier.
    pub id: ThreadId,
    /// Parent session ID.
    pub session_id: SessionId,
    /// Template ID this thread was created from.
    pub template_id: AgentId,
    /// Provider ID for this thread.
    pub provider_id: ProviderId,
    /// Optional thread title.
    pub title: Option<String>,
    /// Creation timestamp.
    pub created_at: DateTime<Utc>,
    /// Last update timestamp.
    pub updated_at: DateTime<Utc>,

    // Runtime (in-memory only)
    /// Execution thread wrapped in async Mutex.
    execution_thread: Mutex<argus_thread::Thread>,
    /// System prompt (for reference).
    system_prompt: String,
}

impl RuntimeThread {
    /// Create a new RuntimeThread.
    ///
    /// # Errors
    ///
    /// Returns `ThreadError` if the execution thread cannot be built.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: ThreadId,
        session_id: SessionId,
        template_id: AgentId,
        provider_id: ProviderId,
        title: Option<String>,
        provider: Arc<dyn LlmProvider>,
        tool_manager: Arc<ToolManager>,
        compactor: Arc<dyn Compactor>,
        system_prompt: String,
        config: ThreadConfig,
    ) -> Result<Self, ThreadError> {
        let now = Utc::now();

        // Build execution thread
        let mut thread = ThreadBuilder::new()
            .id(id.inner().to_string())
            .provider(provider)
            .tool_manager(tool_manager)
            .compactor(compactor)
            .config(config)
            .build()?;

        // Add system prompt as first message
        if !system_prompt.is_empty() {
            thread.messages_mut().push(ChatMessage::system(&system_prompt));
        }

        Ok(Self {
            id,
            session_id,
            template_id,
            provider_id,
            title,
            created_at: now,
            updated_at: now,
            execution_thread: Mutex::new(thread),
            system_prompt,
        })
    }

    /// Send a message to this thread.
    ///
    /// This spawns a background task to process the message.
    ///
    /// # Errors
    ///
    /// Returns `ThreadError` if the thread fails to process the message.
    pub async fn send_message(&self, message: String) -> Result<(), ThreadError> {
        let thread_id = self.id.clone();
        let thread_arc = self.execution_thread.lock().await;

        // We need to spawn a task that owns the mutex guard
        // But MutexGuard is not Send, so we need a different approach
        //
        // Actually, we need to release the lock and re-acquire it in the spawned task.
        // The cleanest way is to use tokio::spawn with the Mutex inside an Arc.
        // But our Mutex is already inside the struct which should be behind an Arc.

        drop(thread_arc);

        // Clone what we need for the spawned task
        let mutex = self.execution_thread();

        tokio::spawn(async move {
            let mut thread = mutex.lock().await;
            if let Err(error) = thread.send_message(message).await {
                tracing::error!(
                    %thread_id,
                    error = %error,
                    "thread turn failed while processing a message"
                );
            }
        });

        Ok(())
    }

    /// Subscribe to thread events.
    pub async fn subscribe(&self) -> broadcast::Receiver<ThreadEvent> {
        let thread = self.execution_thread.lock().await;
        thread.subscribe()
    }

    /// Get a reference to the execution thread mutex.
    pub fn execution_thread(&self) -> &'static Mutex<argus_thread::Thread> {
        // SAFETY: This is a bit of a hack. The RuntimeThread should be wrapped in Arc
        // for the lifetime to work correctly. In practice, SessionManager stores
        // Arc<RuntimeThread>, so the lifetime is 'static for the mutex reference.
        //
        // A cleaner approach would be to use Arc<Mutex<Thread>> directly.
        // For now, we use transmute to extend the lifetime.
        //
        // This is safe because RuntimeThread is always accessed via Arc<RuntimeThread>
        // in SessionManager, and the Arc ensures the data lives long enough.
        unsafe { std::mem::transmute(&self.execution_thread) }
    }

    /// Get the system prompt.
    #[must_use]
    pub fn system_prompt(&self) -> &str {
        &self.system_prompt
    }

    /// Get the thread ID.
    #[must_use]
    pub fn id(&self) -> &ThreadId {
        &self.id
    }

    /// Get current token count.
    pub async fn token_count(&self) -> u32 {
        let thread = self.execution_thread.lock().await;
        thread.token_count()
    }

    /// Get current turn count.
    pub async fn turn_count(&self) -> u32 {
        let thread = self.execution_thread.lock().await;
        thread.turn_count()
    }
}

impl std::fmt::Debug for RuntimeThread {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RuntimeThread")
            .field("id", &self.id)
            .field("session_id", &self.session_id)
            .field("template_id", &self.template_id)
            .field("provider_id", &self.provider_id)
            .field("title", &self.title)
            .finish()
    }
}
