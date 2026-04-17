use std::sync::{Arc, Weak};

use argus_agent::Thread;
use argus_protocol::{
    MailboxMessage, MailboxMessageType, MessageOverride, SessionId, ThreadEvent, ThreadId,
    ThreadMessage,
};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::RwLock;

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
    threads: DashMap<ThreadId, Weak<RwLock<Thread>>>,
}

impl Session {
    pub fn new(id: SessionId, name: String) -> Self {
        Self {
            id,
            name,
            threads: DashMap::new(),
        }
    }

    pub fn add_thread(&self, thread: Arc<RwLock<Thread>>) {
        let thread_arc = Arc::clone(&thread);
        let Ok(thread_guard) = thread.try_read() else {
            return;
        };
        let thread_id = thread_guard.id();
        self.threads.insert(thread_id, Arc::downgrade(&thread_arc));
    }

    pub fn remove_thread(&self, thread_id: &ThreadId) -> Option<Arc<RwLock<Thread>>> {
        self.threads
            .remove(thread_id)
            .and_then(|pair| pair.1.upgrade())
    }

    pub fn get_thread(&self, thread_id: &ThreadId) -> Option<Arc<RwLock<Thread>>> {
        let thread = self
            .threads
            .get(thread_id)
            .and_then(|r| r.value().upgrade());
        if thread.is_none() {
            self.threads.remove(thread_id);
        }
        thread
    }

    async fn send_thread_message(&self, thread_id: &ThreadId, message: ThreadMessage) -> bool {
        let Some(thread) = self.get_thread(thread_id) else {
            return false;
        };
        let delivered = thread.read().await.send_message(message).is_ok();
        delivered
    }

    pub async fn enqueue_user_message(
        &self,
        thread_id: &ThreadId,
        content: String,
        msg_override: Option<MessageOverride>,
    ) -> bool {
        self.send_thread_message(
            thread_id,
            ThreadMessage::UserInput {
                content,
                msg_override,
            },
        )
        .await
    }

    pub async fn enqueue_mailbox_message(
        &self,
        thread_id: &ThreadId,
        message: MailboxMessage,
    ) -> bool {
        let routed = if matches!(message.message_type, MailboxMessageType::JobResult { .. }) {
            ThreadMessage::JobResult { message }
        } else {
            ThreadMessage::PeerMessage { message }
        };
        self.send_thread_message(thread_id, routed).await
    }

    pub async fn interrupt_thread(&self, thread_id: &ThreadId) -> bool {
        self.send_thread_message(thread_id, ThreadMessage::Interrupt)
            .await
    }

    pub fn thread_ids(&self) -> Vec<ThreadId> {
        self.threads.iter().map(|e| *e.key()).collect()
    }

    /// Broadcast a ThreadEvent to all threads in this session.
    pub fn broadcast(&self, event: ThreadEvent) {
        let mut stale_thread_ids = Vec::new();
        for entry in self.threads.iter() {
            let thread = entry.value().upgrade();
            if let Some(thread) = thread {
                match &event {
                    ThreadEvent::UserInterrupt { .. } => {
                        let thread = Arc::clone(&thread);
                        tokio::spawn(async move {
                            let _ = thread.read().await.send_message(ThreadMessage::Interrupt);
                        });
                    }
                    _ => {
                        if let Ok(t) = thread.try_read() {
                            t.broadcast_to_self(event.clone());
                        }
                    }
                }
            } else {
                stale_thread_ids.push(*entry.key());
            }
        }

        for thread_id in stale_thread_ids {
            self.threads.remove(&thread_id);
        }
    }

    pub async fn list_threads(&self) -> Vec<ThreadSummary> {
        let mut summaries = Vec::new();
        let mut stale_thread_ids = Vec::new();
        for entry in self.threads.iter() {
            if let Some(thread) = entry.value().upgrade() {
                if let Ok(t) = thread.try_read() {
                    summaries.push(ThreadSummary {
                        id: t.id(),
                        title: t.title().map(|s| s.to_string()),
                        turn_count: t.turn_count() as i64,
                        token_count: t.token_count() as i64,
                        updated_at: t.updated_at(),
                    });
                }
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
