use std::sync::Arc;

use argus_protocol::{SessionId, ThreadId};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

use crate::RuntimeThread;

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
    pub threads: DashMap<ThreadId, Arc<RuntimeThread>>,
}

impl Session {
    pub fn new(id: SessionId, name: String) -> Self {
        Self {
            id,
            name,
            threads: DashMap::new(),
        }
    }

    pub fn add_thread(&self, thread: Arc<RuntimeThread>) {
        self.threads.insert(thread.id.clone(), thread);
    }

    pub fn remove_thread(&self, thread_id: &ThreadId) -> Option<Arc<RuntimeThread>> {
        self.threads.remove(thread_id).map(|pair| pair.1)
    }

    pub fn get_thread(&self, thread_id: &ThreadId) -> Option<Arc<RuntimeThread>> {
        self.threads.get(thread_id).map(|r| r.value().clone())
    }

    pub async fn list_threads(&self) -> Vec<ThreadSummary> {
        let mut summaries = Vec::new();
        for entry in self.threads.iter() {
            let thread = entry.value();
            let turn_count = thread.turn_count().await as i64;
            let token_count = thread.token_count().await as i64;
            summaries.push(ThreadSummary {
                id: thread.id.clone(),
                title: thread.title.clone(),
                turn_count,
                token_count,
                updated_at: thread.updated_at,
            });
        }
        summaries
    }
}
