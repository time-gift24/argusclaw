use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::future::Future;
use std::sync::{Arc, Mutex};

use argus_protocol::{
    MailboxMessage, RuntimeEventReason, RuntimeKind, RuntimeRef, RuntimeStatus, SessionId,
    ThreadControlEvent, ThreadEvent, ThreadId, ThreadRuntimeSnapshot, ThreadRuntimeState,
    ThreadRuntimeSummary,
};
use chrono::Utc;
use tokio::sync::{Mutex as AsyncMutex, RwLock, broadcast, mpsc};
use tokio::task::AbortHandle;

#[derive(Debug)]
struct RuntimeEntry {
    summary: ThreadRuntimeSummary,
    sender: broadcast::Sender<ThreadEvent>,
    thread: Option<Arc<RwLock<crate::Thread>>>,
    control_tx: Option<mpsc::UnboundedSender<ThreadControlEvent>>,
    forwarder_abort: Option<AbortHandle>,
    load_mutex: Arc<AsyncMutex<()>>,
}

#[derive(Debug, Default)]
struct ThreadRuntimeStore {
    runtimes: HashMap<ThreadId, RuntimeEntry>,
    parent_thread_by_child: HashMap<ThreadId, ThreadId>,
    child_threads_by_parent: HashMap<ThreadId, Vec<ThreadId>>,
}

#[derive(Debug, Default)]
struct RuntimeShutdown {
    thread: Option<Arc<RwLock<crate::Thread>>>,
    control_tx: Option<mpsc::UnboundedSender<ThreadControlEvent>>,
    forwarder_abort: Option<AbortHandle>,
}

impl RuntimeShutdown {
    fn run(self) {
        if let Some(forwarder_abort) = self.forwarder_abort {
            forwarder_abort.abort();
        }
        if let Some(control_tx) = self.control_tx {
            let _ = control_tx.send(ThreadControlEvent::ShutdownRuntime);
        }
        drop(self.thread);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadRegistration {
    pub thread_id: ThreadId,
    pub kind: RuntimeKind,
    pub session_id: Option<SessionId>,
    pub parent_thread_id: Option<ThreadId>,
    pub job_id: Option<String>,
    pub recoverable: bool,
}

#[derive(Debug, Default)]
pub struct ThreadRuntime {
    store: Arc<Mutex<ThreadRuntimeStore>>,
}

impl ThreadRuntime {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register_thread(&self, registration: ThreadRegistration) {
        let mut store = self.store.lock().expect("thread-runtime mutex poisoned");
        Self::sync_relationship_cache(&mut store, &registration);
        Self::upsert_runtime_summary(&mut store, registration);
    }

    pub async fn ensure_chat_runtime<F, Fut>(
        &self,
        registration: ThreadRegistration,
        load_thread: F,
    ) -> Result<Arc<RwLock<crate::Thread>>, String>
    where
        F: FnOnce() -> Fut,
        Fut: Future<Output = Result<Arc<RwLock<crate::Thread>>, String>>,
    {
        self.register_thread(registration.clone());

        if let Some(thread) = self.loaded_chat_thread(&registration.thread_id) {
            return Ok(thread);
        }

        let load_mutex = self.runtime_load_mutex(&registration.thread_id)?;
        let _load_guard = load_mutex.lock().await;
        if let Some(thread) = self.loaded_chat_thread(&registration.thread_id) {
            return Ok(thread);
        }

        self.mark_runtime_loading(&registration.thread_id)?;
        let thread = match load_thread().await {
            Ok(thread) => thread,
            Err(error) => {
                self.reset_runtime_after_load_failure(&registration.thread_id);
                return Err(error);
            }
        };
        let runtime_rx = {
            let guard = thread.read().await;
            guard.subscribe()
        };
        crate::Thread::spawn_reactor(Arc::clone(&thread)).await;
        if let Err(error) = self
            .attach_loaded_thread(registration.thread_id, Arc::clone(&thread), runtime_rx)
            .await
        {
            self.reset_runtime_after_load_failure(&registration.thread_id);
            return Err(error);
        }

        Ok(thread)
    }

    #[must_use]
    pub fn subscribe(&self, thread_id: &ThreadId) -> Option<broadcast::Receiver<ThreadEvent>> {
        self.store
            .lock()
            .expect("thread-runtime mutex poisoned")
            .runtimes
            .get(thread_id)
            .map(|entry| entry.sender.subscribe())
    }

    #[must_use]
    pub fn runtime_summary(&self, thread_id: &ThreadId) -> Option<ThreadRuntimeSummary> {
        self.store
            .lock()
            .expect("thread-runtime mutex poisoned")
            .runtimes
            .get(thread_id)
            .map(|entry| entry.summary.clone())
    }

    #[must_use]
    pub fn collect_state(&self) -> ThreadRuntimeState {
        let store = self.store.lock().expect("thread-runtime mutex poisoned");
        let runtimes: Vec<_> = store
            .runtimes
            .values()
            .filter(|entry| entry.summary.runtime.kind == RuntimeKind::Chat)
            .map(|entry| entry.summary.clone())
            .collect();
        let queued_threads = runtimes
            .iter()
            .filter(|runtime| runtime.status == RuntimeStatus::Queued)
            .count() as u32;
        let running_threads = runtimes
            .iter()
            .filter(|runtime| runtime.status == RuntimeStatus::Running)
            .count() as u32;
        let cooling_threads = runtimes
            .iter()
            .filter(|runtime| runtime.status == RuntimeStatus::Cooling)
            .count() as u32;
        let resident_thread_count = store
            .runtimes
            .values()
            .filter(|entry| entry.thread.is_some())
            .count() as u32;
        let estimated_memory_bytes = store
            .runtimes
            .values()
            .filter(|entry| entry.thread.is_some())
            .map(|entry| entry.summary.estimated_memory_bytes)
            .sum::<u64>();
        let avg_thread_memory_bytes = if resident_thread_count == 0 {
            0
        } else {
            estimated_memory_bytes / u64::from(resident_thread_count)
        };

        ThreadRuntimeState {
            snapshot: ThreadRuntimeSnapshot {
                // ThreadRuntime is a registry view and does not own job admission limits.
                max_threads: 0,
                active_threads: resident_thread_count,
                queued_threads,
                running_threads,
                cooling_threads,
                evicted_threads: runtimes
                    .iter()
                    .filter(|runtime| runtime.status == RuntimeStatus::Evicted)
                    .count() as u64,
                estimated_memory_bytes,
                peak_estimated_memory_bytes: estimated_memory_bytes,
                process_memory_bytes: None,
                peak_process_memory_bytes: None,
                // Registry residency counts loaded runtimes, not job-runtime slot ownership.
                resident_thread_count,
                avg_thread_memory_bytes,
                captured_at: Utc::now().to_rfc3339(),
            },
            runtimes,
        }
    }

    #[must_use]
    pub fn loaded_chat_thread(&self, thread_id: &ThreadId) -> Option<Arc<RwLock<crate::Thread>>> {
        self.store
            .lock()
            .expect("thread-runtime mutex poisoned")
            .runtimes
            .get(thread_id)
            .and_then(|entry| {
                (entry.summary.runtime.kind == RuntimeKind::Chat)
                    .then(|| entry.thread.clone())
                    .flatten()
            })
    }

    #[must_use]
    pub fn loaded_thread(&self, thread_id: &ThreadId) -> Option<Arc<RwLock<crate::Thread>>> {
        self.store
            .lock()
            .expect("thread-runtime mutex poisoned")
            .runtimes
            .get(thread_id)
            .and_then(|entry| entry.thread.clone())
    }

    pub fn remove_runtime(&self, thread_id: &ThreadId) -> bool {
        let mut store = self.store.lock().expect("thread-runtime mutex poisoned");
        let removed_entry = store.runtimes.remove(thread_id);
        let removed = removed_entry.is_some();
        if removed {
            if let Some(parent_thread_id) = store.parent_thread_by_child.remove(thread_id)
                && let Some(children) = store.child_threads_by_parent.get_mut(&parent_thread_id)
            {
                children.retain(|child_thread_id| child_thread_id != thread_id);
                if children.is_empty() {
                    store.child_threads_by_parent.remove(&parent_thread_id);
                }
            }
            if let Some(child_thread_ids) = store.child_threads_by_parent.remove(thread_id) {
                for child_thread_id in child_thread_ids {
                    store.parent_thread_by_child.remove(&child_thread_id);
                }
            }
        }
        drop(store);

        if let Some(entry) = removed_entry {
            RuntimeShutdown {
                thread: entry.thread,
                control_tx: entry.control_tx,
                forwarder_abort: entry.forwarder_abort,
            }
            .run();
        }

        removed
    }

    pub async fn unread_mailbox_messages(
        &self,
        thread_id: ThreadId,
    ) -> Result<Vec<MailboxMessage>, String> {
        let thread = self
            .loaded_thread(&thread_id)
            .ok_or_else(|| format!("thread {} is not loaded", thread_id))?;
        let mailbox = {
            let guard = thread.read().await;
            guard.mailbox()
        };

        Ok(mailbox.lock().await.unread_mailbox_messages())
    }

    pub async fn mark_mailbox_message_read(
        &self,
        thread_id: ThreadId,
        message_id: &str,
    ) -> Result<bool, String> {
        let thread = self
            .loaded_thread(&thread_id)
            .ok_or_else(|| format!("thread {} is not loaded", thread_id))?;
        let mailbox = {
            let guard = thread.read().await;
            guard.mailbox()
        };

        Ok(mailbox.lock().await.mark_mailbox_message_read(message_id))
    }

    pub async fn deliver_mailbox_message(
        &self,
        thread_id: ThreadId,
        message: MailboxMessage,
    ) -> Result<(), String> {
        let (thread, sender) = {
            let store = self.store.lock().expect("thread-runtime mutex poisoned");
            let entry = store
                .runtimes
                .get(&thread_id)
                .ok_or_else(|| format!("thread {} is not registered", thread_id))?;
            let thread = entry
                .thread
                .clone()
                .ok_or_else(|| format!("thread {} is not loaded", thread_id))?;
            (thread, entry.sender.clone())
        };
        let (mailbox, control_tx) = {
            let guard = thread.read().await;
            (guard.mailbox(), guard.control_tx())
        };

        mailbox
            .lock()
            .await
            .enqueue_mailbox_message(message.clone());
        let _ = control_tx.send(ThreadControlEvent::MailboxUpdated);
        let _ = sender.send(ThreadEvent::MailboxMessageQueued { thread_id, message });
        Ok(())
    }

    fn runtime_load_mutex(&self, thread_id: &ThreadId) -> Result<Arc<AsyncMutex<()>>, String> {
        self.store
            .lock()
            .expect("thread-runtime mutex poisoned")
            .runtimes
            .get(thread_id)
            .map(|entry| Arc::clone(&entry.load_mutex))
            .ok_or_else(|| format!("thread {} is not registered", thread_id))
    }

    fn mark_runtime_loading(&self, thread_id: &ThreadId) -> Result<(), String> {
        let mut store = self.store.lock().expect("thread-runtime mutex poisoned");
        let entry = store
            .runtimes
            .get_mut(thread_id)
            .ok_or_else(|| format!("thread {} is not registered", thread_id))?;
        entry.summary.status = RuntimeStatus::Loading;
        entry.summary.last_active_at = Some(Utc::now().to_rfc3339());
        entry.summary.last_reason = None;
        Ok(())
    }

    fn reset_runtime_after_load_failure(&self, thread_id: &ThreadId) {
        let mut store = self.store.lock().expect("thread-runtime mutex poisoned");
        let mut shutdown = RuntimeShutdown::default();
        if let Some(entry) = store.runtimes.get_mut(thread_id) {
            entry.summary.status = RuntimeStatus::Inactive;
            entry.summary.estimated_memory_bytes = 0;
            entry.summary.last_active_at = Some(Utc::now().to_rfc3339());
            entry.summary.last_reason = None;
            shutdown = Self::take_runtime_shutdown(entry);
        }
        drop(store);
        shutdown.run();
    }

    async fn attach_loaded_thread(
        &self,
        thread_id: ThreadId,
        thread: Arc<RwLock<crate::Thread>>,
        runtime_rx: broadcast::Receiver<ThreadEvent>,
    ) -> Result<(), String> {
        let control_tx = {
            let guard = thread.read().await;
            guard.control_tx()
        };
        let (sender, replaced_runtime) = {
            let mut store = self.store.lock().expect("thread-runtime mutex poisoned");
            let (sender, replaced_runtime) = {
                let Some(entry) = store.runtimes.get_mut(&thread_id) else {
                    return Err(format!("thread {} was removed while loading", thread_id));
                };
                let replaced_runtime = if entry
                    .thread
                    .as_ref()
                    .is_some_and(|existing| !Arc::ptr_eq(existing, &thread))
                {
                    Self::take_runtime_shutdown(entry)
                } else {
                    RuntimeShutdown::default()
                };
                entry.summary.status = RuntimeStatus::Inactive;
                entry.summary.last_active_at = Some(Utc::now().to_rfc3339());
                entry.summary.last_reason = None;
                entry.thread = Some(Arc::clone(&thread));
                entry.control_tx = Some(control_tx.clone());
                entry.forwarder_abort = None;
                (entry.sender.clone(), replaced_runtime)
            };
            (sender, replaced_runtime)
        };
        replaced_runtime.run();

        let store = Arc::clone(&self.store);
        let mut runtime_rx = runtime_rx.resubscribe();
        let forwarder = tokio::spawn(async move {
            loop {
                match runtime_rx.recv().await {
                    Ok(event) => {
                        ThreadRuntime::apply_runtime_event(&store, &thread_id, &event);
                        let _ = sender.send(event);
                    }
                    Err(broadcast::error::RecvError::Closed) => break,
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                }
            }
        });
        let forwarder_abort = forwarder.abort_handle();
        let mut store = self.store.lock().expect("thread-runtime mutex poisoned");
        let Some(entry) = store.runtimes.get_mut(&thread_id) else {
            forwarder_abort.abort();
            return Err(format!("thread {} was removed while attaching", thread_id));
        };
        entry.forwarder_abort = Some(forwarder_abort);
        Ok(())
    }

    fn apply_runtime_event(
        store: &Arc<Mutex<ThreadRuntimeStore>>,
        thread_id: &ThreadId,
        event: &ThreadEvent,
    ) {
        let mut store = store.lock().expect("thread-runtime mutex poisoned");
        let Some(entry) = store.runtimes.get_mut(thread_id) else {
            return;
        };

        let (status, last_reason) = match event {
            ThreadEvent::Processing { .. }
            | ThreadEvent::ToolStarted { .. }
            | ThreadEvent::ToolCompleted { .. }
            | ThreadEvent::CompactionStarted { .. } => (Some(RuntimeStatus::Running), None),
            ThreadEvent::Idle { .. }
            | ThreadEvent::TurnCompleted { .. }
            | ThreadEvent::TurnSettled { .. }
            | ThreadEvent::CompactionFinished { .. }
            | ThreadEvent::Compacted { .. } => (Some(RuntimeStatus::Inactive), None),
            ThreadEvent::TurnFailed { .. } | ThreadEvent::CompactionFailed { .. } => (
                Some(RuntimeStatus::Inactive),
                Some(RuntimeEventReason::ExecutionFailed),
            ),
            _ => (None, None),
        };

        if let Some(status) = status {
            entry.summary.status = status;
            entry.summary.last_active_at = Some(Utc::now().to_rfc3339());
            entry.summary.last_reason = last_reason;
        }
    }

    fn take_runtime_shutdown(entry: &mut RuntimeEntry) -> RuntimeShutdown {
        RuntimeShutdown {
            thread: entry.thread.take(),
            control_tx: entry.control_tx.take(),
            forwarder_abort: entry.forwarder_abort.take(),
        }
    }

    fn sync_relationship_cache(store: &mut ThreadRuntimeStore, registration: &ThreadRegistration) {
        if let Some(previous_parent_thread_id) = store
            .parent_thread_by_child
            .get(&registration.thread_id)
            .copied()
            && Some(previous_parent_thread_id) != registration.parent_thread_id
        {
            let mut remove_parent_entry = false;
            if let Some(children) = store
                .child_threads_by_parent
                .get_mut(&previous_parent_thread_id)
            {
                children.retain(|child_thread_id| child_thread_id != &registration.thread_id);
                remove_parent_entry = children.is_empty();
            }
            if remove_parent_entry {
                store
                    .child_threads_by_parent
                    .remove(&previous_parent_thread_id);
            }
        }

        if let Some(parent_thread_id) = registration.parent_thread_id {
            store
                .parent_thread_by_child
                .insert(registration.thread_id, parent_thread_id);
            let children = store
                .child_threads_by_parent
                .entry(parent_thread_id)
                .or_default();
            if !children.contains(&registration.thread_id) {
                children.push(registration.thread_id);
            }
        } else {
            store.parent_thread_by_child.remove(&registration.thread_id);
        }
    }

    fn upsert_runtime_summary(store: &mut ThreadRuntimeStore, registration: ThreadRegistration) {
        let summary = ThreadRuntimeSummary {
            runtime: RuntimeRef {
                thread_id: registration.thread_id,
                kind: registration.kind,
                session_id: registration.session_id,
                job_id: registration.job_id,
            },
            status: RuntimeStatus::Inactive,
            estimated_memory_bytes: 0,
            last_active_at: None,
            recoverable: registration.recoverable,
            last_reason: None,
        };

        match store.runtimes.entry(registration.thread_id) {
            Entry::Occupied(mut entry) => {
                let current = entry.get_mut();
                current.summary.runtime = summary.runtime;
                current.summary.recoverable = summary.recoverable;
            }
            Entry::Vacant(entry) => {
                let (sender, _) = broadcast::channel(128);
                entry.insert(RuntimeEntry {
                    summary,
                    sender,
                    thread: None,
                    control_tx: None,
                    forwarder_abort: None,
                    load_mutex: Arc::new(AsyncMutex::new(())),
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::{LlmThreadCompactor, Thread, ThreadBuilder};
    use argus_protocol::llm::{
        CompletionRequest, CompletionResponse, FinishReason, LlmError, LlmProvider,
    };
    use argus_protocol::{
        AgentId, AgentRecord, RuntimeEventReason, RuntimeKind, RuntimeStatus, SessionId,
        ThinkingConfig, ThreadEvent, ThreadId,
    };
    use tokio::sync::RwLock;
    use tokio::time::{Duration, timeout};

    use super::{ThreadRegistration, ThreadRuntime};

    #[derive(Debug)]
    struct NoopProvider;

    #[async_trait::async_trait]
    impl LlmProvider for NoopProvider {
        fn model_name(&self) -> &str {
            "noop"
        }

        fn cost_per_token(&self) -> (rust_decimal::Decimal, rust_decimal::Decimal) {
            (rust_decimal::Decimal::ZERO, rust_decimal::Decimal::ZERO)
        }

        async fn complete(
            &self,
            _request: CompletionRequest,
        ) -> std::result::Result<CompletionResponse, LlmError> {
            Ok(CompletionResponse {
                content: Some("ok".to_string()),
                reasoning_content: None,
                tool_calls: Vec::new(),
                input_tokens: 1,
                output_tokens: 1,
                finish_reason: FinishReason::Stop,
                cache_read_input_tokens: 0,
                cache_creation_input_tokens: 0,
            })
        }

        async fn stream_complete(
            &self,
            _request: CompletionRequest,
        ) -> std::result::Result<argus_protocol::llm::LlmEventStream, LlmError> {
            Err(LlmError::UnsupportedCapability {
                provider: self.model_name().to_string(),
                capability: "stream_complete".to_string(),
            })
        }
    }

    async fn attach_test_chat_thread(
        runtime: &ThreadRuntime,
        thread_id: ThreadId,
        session_id: SessionId,
    ) -> Arc<RwLock<Thread>> {
        runtime.register_thread(ThreadRegistration {
            thread_id,
            kind: RuntimeKind::Chat,
            session_id: Some(session_id),
            parent_thread_id: None,
            job_id: None,
            recoverable: true,
        });

        let provider: Arc<dyn LlmProvider> = Arc::new(NoopProvider);
        let thread = Arc::new(RwLock::new(
            ThreadBuilder::new()
                .id(thread_id)
                .session_id(session_id)
                .agent_record(Arc::new(AgentRecord {
                    id: AgentId::new(1),
                    display_name: "Runtime Test Agent".to_string(),
                    description: "Used to test runtime summaries".to_string(),
                    version: "1.0.0".to_string(),
                    provider_id: None,
                    model_id: Some("noop".to_string()),
                    system_prompt: "Observe runtime state.".to_string(),
                    tool_names: vec![],
                    subagent_names: vec![],
                    max_tokens: None,
                    temperature: None,
                    thinking_config: Some(ThinkingConfig::disabled()),
                }))
                .provider(Arc::clone(&provider))
                .compactor(Arc::new(LlmThreadCompactor::new(provider)))
                .build()
                .expect("thread should build"),
        ));
        let runtime_rx = {
            let guard = thread.read().await;
            guard.subscribe()
        };
        runtime
            .attach_loaded_thread(thread_id, Arc::clone(&thread), runtime_rx)
            .await
            .expect("thread should attach");
        thread
    }

    async fn wait_for_status(
        runtime: &ThreadRuntime,
        thread_id: ThreadId,
        expected: RuntimeStatus,
    ) {
        timeout(Duration::from_secs(2), async {
            loop {
                if runtime
                    .runtime_summary(&thread_id)
                    .is_some_and(|summary| summary.status == expected)
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("runtime summary should update");
    }

    #[test]
    fn register_thread_exposes_subscription_and_summary() {
        let runtime = ThreadRuntime::new();
        let thread_id = ThreadId::new();
        let session_id = SessionId::new();

        runtime.register_thread(ThreadRegistration {
            thread_id,
            kind: RuntimeKind::Chat,
            session_id: Some(session_id),
            parent_thread_id: None,
            job_id: None,
            recoverable: true,
        });

        let summary = runtime
            .runtime_summary(&thread_id)
            .expect("registered thread should expose a summary");

        assert_eq!(summary.runtime.thread_id, thread_id);
        assert_eq!(summary.runtime.kind, RuntimeKind::Chat);
        assert_eq!(summary.runtime.session_id, Some(session_id));
        assert_eq!(summary.runtime.job_id, None);
        assert_eq!(summary.status, RuntimeStatus::Inactive);
        assert!(runtime.subscribe(&thread_id).is_some());
    }

    #[test]
    fn register_thread_updates_summary_for_existing_thread() {
        let runtime = ThreadRuntime::new();
        let thread_id = ThreadId::new();
        let first_session_id = SessionId::new();
        let second_session_id = SessionId::new();

        runtime.register_thread(ThreadRegistration {
            thread_id,
            kind: RuntimeKind::Chat,
            session_id: Some(first_session_id),
            parent_thread_id: None,
            job_id: None,
            recoverable: true,
        });

        runtime.register_thread(ThreadRegistration {
            thread_id,
            kind: RuntimeKind::Job,
            session_id: Some(second_session_id),
            parent_thread_id: None,
            job_id: Some("job-123".to_string()),
            recoverable: false,
        });

        let summary = runtime
            .runtime_summary(&thread_id)
            .expect("re-registered thread should update its summary");

        assert_eq!(summary.runtime.kind, RuntimeKind::Job);
        assert_eq!(summary.runtime.session_id, Some(second_session_id));
        assert_eq!(summary.runtime.job_id.as_deref(), Some("job-123"));
        assert!(!summary.recoverable);
        assert!(runtime.subscribe(&thread_id).is_some());
    }

    #[test]
    fn register_thread_preserves_existing_runtime_state() {
        let runtime = ThreadRuntime::new();
        let thread_id = ThreadId::new();
        let first_session_id = SessionId::new();
        let second_session_id = SessionId::new();

        runtime.register_thread(ThreadRegistration {
            thread_id,
            kind: RuntimeKind::Chat,
            session_id: Some(first_session_id),
            parent_thread_id: None,
            job_id: None,
            recoverable: true,
        });

        {
            let mut store = runtime.store.lock().expect("thread-runtime mutex poisoned");
            let entry = store
                .runtimes
                .get_mut(&thread_id)
                .expect("thread should be registered");
            entry.summary.status = RuntimeStatus::Running;
            entry.summary.estimated_memory_bytes = 512;
            entry.summary.last_active_at = Some("2026-04-12T12:34:56Z".to_string());
            entry.summary.last_reason = Some(RuntimeEventReason::CoolingExpired);
        }

        runtime.register_thread(ThreadRegistration {
            thread_id,
            kind: RuntimeKind::Job,
            session_id: Some(second_session_id),
            parent_thread_id: None,
            job_id: Some("job-456".to_string()),
            recoverable: false,
        });

        let summary = runtime
            .runtime_summary(&thread_id)
            .expect("re-registered thread should still expose a summary");

        assert_eq!(summary.runtime.kind, RuntimeKind::Job);
        assert_eq!(summary.runtime.session_id, Some(second_session_id));
        assert_eq!(summary.runtime.job_id.as_deref(), Some("job-456"));
        assert_eq!(summary.status, RuntimeStatus::Running);
        assert_eq!(summary.estimated_memory_bytes, 512);
        assert_eq!(
            summary.last_active_at.as_deref(),
            Some("2026-04-12T12:34:56Z")
        );
        assert_eq!(
            summary.last_reason,
            Some(RuntimeEventReason::CoolingExpired)
        );
        assert!(!summary.recoverable);
    }

    #[tokio::test]
    async fn attached_chat_runtime_summary_tracks_processing_and_idle_events() {
        let runtime = ThreadRuntime::new();
        let thread_id = ThreadId::new();
        let session_id = SessionId::new();
        let thread = attach_test_chat_thread(&runtime, thread_id, session_id).await;

        thread
            .read()
            .await
            .broadcast_to_self(ThreadEvent::Processing {
                thread_id: thread_id.to_string(),
                turn_number: 1,
                event: argus_protocol::llm::LlmStreamEvent::ContentDelta {
                    delta: "hello".to_string(),
                },
            });
        wait_for_status(&runtime, thread_id, RuntimeStatus::Running).await;

        thread.read().await.broadcast_to_self(ThreadEvent::Idle {
            thread_id: thread_id.to_string(),
        });
        wait_for_status(&runtime, thread_id, RuntimeStatus::Inactive).await;
    }

    #[tokio::test]
    async fn attached_chat_runtime_summary_marks_failures_in_last_reason() {
        let runtime = ThreadRuntime::new();
        let thread_id = ThreadId::new();
        let session_id = SessionId::new();
        let thread = attach_test_chat_thread(&runtime, thread_id, session_id).await;

        thread
            .read()
            .await
            .broadcast_to_self(ThreadEvent::TurnFailed {
                thread_id: thread_id.to_string(),
                turn_number: 1,
                error: "boom".to_string(),
            });

        timeout(Duration::from_secs(2), async {
            loop {
                if let Some(summary) = runtime.runtime_summary(&thread_id)
                    && summary.status == RuntimeStatus::Inactive
                    && summary.last_reason == Some(RuntimeEventReason::ExecutionFailed)
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("runtime failure summary should update");
    }
}
