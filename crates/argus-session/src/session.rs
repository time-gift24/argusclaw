use argus_agent::{ThreadHandle, WeakThreadHandle};
use argus_protocol::{SessionId, ThreadId, ThreadMessage};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};

/// Summary of a session for listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: SessionId,
    pub name: String,
    pub thread_count: i64,
    pub updated_at: DateTime<Utc>,
}

/// Thread summary for listing.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadSummary {
    pub id: ThreadId,
    pub title: Option<String>,
    pub turn_count: i64,
    pub token_count: i64,
    pub updated_at: DateTime<Utc>,
}

/// Session - container for multiple threads.
pub struct Session {
    pub id: SessionId,
    pub name: String,
    threads: DashMap<ThreadId, WeakThreadHandle>,
}

impl Session {
    pub fn new(id: SessionId, name: String) -> Self {
        Self {
            id,
            name,
            threads: DashMap::new(),
        }
    }

    pub fn add_thread(&self, thread: ThreadHandle) {
        let thread_id = thread.id();
        self.threads.insert(thread_id, thread.downgrade());
    }

    pub fn remove_thread(&self, thread_id: &ThreadId) -> Option<ThreadHandle> {
        self.threads
            .remove(thread_id)
            .and_then(|pair| pair.1.upgrade())
    }

    pub fn get_thread(&self, thread_id: &ThreadId) -> Option<ThreadHandle> {
        let thread = self
            .threads
            .get(thread_id)
            .and_then(|r| r.value().upgrade());
        if thread.is_none() {
            self.threads.remove(thread_id);
        }
        thread
    }

    fn send_thread_message(&self, thread_id: &ThreadId, message: ThreadMessage) -> bool {
        let Some(thread) = self.get_thread(thread_id) else {
            return false;
        };
        thread.send_message(message).is_ok()
    }

    pub async fn interrupt_thread(&self, thread_id: &ThreadId) -> bool {
        self.send_thread_message(thread_id, ThreadMessage::Interrupt)
    }

    pub fn thread_ids(&self) -> Vec<ThreadId> {
        self.threads.iter().map(|e| *e.key()).collect()
    }

    pub async fn list_threads(&self) -> Vec<ThreadSummary> {
        let mut summaries = Vec::new();
        let mut stale_thread_ids = Vec::new();
        for entry in self.threads.iter() {
            if let Some(thread) = entry.value().upgrade() {
                summaries.push(ThreadSummary {
                    id: thread.id(),
                    title: thread.title(),
                    turn_count: thread.turn_count() as i64,
                    token_count: thread.token_count() as i64,
                    updated_at: thread.updated_at(),
                });
            } else {
                stale_thread_ids.push(*entry.key());
            }
        }

        for thread_id in stale_thread_ids {
            self.threads.remove(&thread_id);
        }
        summaries
    }
}
