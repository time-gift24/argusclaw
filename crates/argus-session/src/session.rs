use argus_protocol::{SessionId, ThreadId, AgentId, ProviderId};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};

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
    pub threads: DashMap<ThreadId, Thread>,
}

/// Thread within a session.
#[derive(Clone)]
pub struct Thread {
    pub id: ThreadId,
    pub session_id: SessionId,
    pub template_id: AgentId,
    pub provider_id: ProviderId,
    pub title: Option<String>,
    pub token_count: u32,
    pub turn_count: u32,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl Session {
    pub fn new(id: SessionId, name: String) -> Self {
        Self {
            id,
            name,
            threads: DashMap::new(),
        }
    }

    pub fn add_thread(&self, thread: Thread) {
        self.threads.insert(thread.id.clone(), thread);
    }

    pub fn remove_thread(&self, thread_id: &ThreadId) -> Option<Thread> {
        self.threads.remove(thread_id).map(|pair| pair.1)
    }

    pub fn get_thread(&self, thread_id: &ThreadId) -> Option<Thread> {
        self.threads.get(thread_id).map(|r| r.value().clone())
    }

    pub fn list_threads(&self) -> Vec<ThreadSummary> {
        self.threads
            .iter()
            .map(|r| {
                let thread = r.value();
                ThreadSummary {
                    id: thread.id.clone(),
                    title: thread.title.clone(),
                    turn_count: thread.turn_count as i64,
                    token_count: thread.token_count as i64,
                    updated_at: thread.updated_at,
                }
            })
            .collect()
    }
}

impl Thread {
    pub fn new(
        id: ThreadId,
        session_id: SessionId,
        template_id: AgentId,
        provider_id: ProviderId,
    ) -> Self {
        let now = Utc::now();
        Self {
            id,
            session_id,
            template_id,
            provider_id,
            title: None,
            token_count: 0,
            turn_count: 0,
            created_at: now,
            updated_at: now,
        }
    }
}
