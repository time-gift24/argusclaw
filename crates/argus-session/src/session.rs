use std::sync::Arc;

use argus_protocol::{SessionId, ThreadId};
use argus_thread::Thread;
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::Mutex;

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
    threads: DashMap<ThreadId, Arc<Mutex<Thread>>>,
}

impl Session {
    pub fn new(id: SessionId, name: String) -> Self {
        Self {
            id,
            name,
            threads: DashMap::new(),
        }
    }

    pub fn add_thread(&self, thread: Arc<Mutex<Thread>>) {
        let thread_id = thread.try_lock().map(|t| t.id()).unwrap_or_default();
        self.threads.insert(thread_id, thread);
    }

    pub fn remove_thread(&self, thread_id: &ThreadId) -> Option<Arc<Mutex<Thread>>> {
        self.threads.remove(thread_id).map(|pair| pair.1)
    }

    pub fn get_thread(&self, thread_id: &ThreadId) -> Option<Arc<Mutex<Thread>>> {
        self.threads.get(thread_id).map(|r| r.value().clone())
    }

    pub fn thread_ids(&self) -> Vec<ThreadId> {
        self.threads.iter().map(|e| *e.key()).collect()
    }

    pub async fn list_threads(&self) -> Vec<ThreadSummary> {
        let mut summaries = Vec::new();
        for entry in self.threads.iter() {
            let thread = entry.value();
            if let Ok(t) = thread.try_lock() {
                summaries.push(ThreadSummary {
                    id: t.id(),
                    title: t.title().map(|s| s.to_string()),
                    turn_count: t.turn_count() as i64,
                    token_count: t.token_count() as i64,
                    updated_at: t.updated_at(),
                });
            }
        }
        summaries
    }
}
