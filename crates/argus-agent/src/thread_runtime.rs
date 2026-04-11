use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::sync::{Arc, Mutex};

use argus_protocol::{
    SessionId, ThreadEvent, ThreadId, ThreadPoolRuntimeKind, ThreadPoolRuntimeRef,
    ThreadPoolRuntimeSummary, ThreadRuntimeStatus,
};
use tokio::sync::broadcast;

#[derive(Debug)]
struct RuntimeEntry {
    summary: ThreadPoolRuntimeSummary,
    sender: broadcast::Sender<ThreadEvent>,
}

#[derive(Debug, Default)]
struct ThreadRuntimeStore {
    runtimes: HashMap<ThreadId, RuntimeEntry>,
    parent_thread_by_child: HashMap<ThreadId, ThreadId>,
    child_threads_by_parent: HashMap<ThreadId, Vec<ThreadId>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ThreadRegistration {
    pub thread_id: ThreadId,
    pub kind: ThreadPoolRuntimeKind,
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
    pub fn runtime_summary(&self, thread_id: &ThreadId) -> Option<ThreadPoolRuntimeSummary> {
        self.store
            .lock()
            .expect("thread-runtime mutex poisoned")
            .runtimes
            .get(thread_id)
            .map(|entry| entry.summary.clone())
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
        let summary = ThreadPoolRuntimeSummary {
            runtime: ThreadPoolRuntimeRef {
                thread_id: registration.thread_id,
                kind: registration.kind,
                session_id: registration.session_id,
                job_id: registration.job_id,
            },
            status: ThreadRuntimeStatus::Inactive,
            estimated_memory_bytes: 0,
            last_active_at: None,
            recoverable: registration.recoverable,
            last_reason: None,
        };

        match store.runtimes.entry(registration.thread_id) {
            Entry::Occupied(mut entry) => {
                entry.get_mut().summary = summary;
            }
            Entry::Vacant(entry) => {
                let (sender, _) = broadcast::channel(128);
                entry.insert(RuntimeEntry { summary, sender });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use argus_protocol::{SessionId, ThreadId, ThreadPoolRuntimeKind, ThreadRuntimeStatus};

    use super::{ThreadRegistration, ThreadRuntime};

    #[test]
    fn register_thread_exposes_subscription_and_summary() {
        let runtime = ThreadRuntime::new();
        let thread_id = ThreadId::new();
        let session_id = SessionId::new();

        runtime.register_thread(ThreadRegistration {
            thread_id,
            kind: ThreadPoolRuntimeKind::Chat,
            session_id: Some(session_id),
            parent_thread_id: None,
            job_id: None,
            recoverable: true,
        });

        let summary = runtime
            .runtime_summary(&thread_id)
            .expect("registered thread should expose a summary");

        assert_eq!(summary.runtime.thread_id, thread_id);
        assert_eq!(summary.runtime.kind, ThreadPoolRuntimeKind::Chat);
        assert_eq!(summary.runtime.session_id, Some(session_id));
        assert_eq!(summary.runtime.job_id, None);
        assert_eq!(summary.status, ThreadRuntimeStatus::Inactive);
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
            kind: ThreadPoolRuntimeKind::Chat,
            session_id: Some(first_session_id),
            parent_thread_id: None,
            job_id: None,
            recoverable: true,
        });

        runtime.register_thread(ThreadRegistration {
            thread_id,
            kind: ThreadPoolRuntimeKind::Job,
            session_id: Some(second_session_id),
            parent_thread_id: None,
            job_id: Some("job-123".to_string()),
            recoverable: false,
        });

        let summary = runtime
            .runtime_summary(&thread_id)
            .expect("re-registered thread should update its summary");

        assert_eq!(summary.runtime.kind, ThreadPoolRuntimeKind::Job);
        assert_eq!(summary.runtime.session_id, Some(second_session_id));
        assert_eq!(summary.runtime.job_id.as_deref(), Some("job-123"));
        assert!(!summary.recoverable);
        assert!(runtime.subscribe(&thread_id).is_some());
    }
}
