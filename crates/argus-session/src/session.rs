use std::sync::{Arc, Weak};

use argus_agent::Thread;
use argus_protocol::{
    MailboxMessage, MessageOverride, SessionId, ThreadEvent, ThreadId, ThreadMailbox,
};
use chrono::{DateTime, Utc};
use dashmap::DashMap;
use serde::{Deserialize, Serialize};
use tokio::sync::{Mutex, RwLock};

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
    mailboxes: DashMap<ThreadId, Arc<Mutex<ThreadMailbox>>>,
}

impl Session {
    pub fn new(id: SessionId, name: String) -> Self {
        Self {
            id,
            name,
            threads: DashMap::new(),
            mailboxes: DashMap::new(),
        }
    }

    pub fn add_thread(&self, thread: Arc<RwLock<Thread>>) {
        let thread_arc = Arc::clone(&thread);
        let Ok(thread_guard) = thread.try_read() else {
            return;
        };
        let thread_id = thread_guard.id();
        let mailbox = thread_guard.mailbox();
        self.threads.insert(thread_id, Arc::downgrade(&thread_arc));
        self.mailboxes.insert(thread_id, mailbox);
    }

    pub fn remove_thread(&self, thread_id: &ThreadId) -> Option<Arc<RwLock<Thread>>> {
        self.mailboxes.remove(thread_id);
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
            self.mailboxes.remove(thread_id);
        }
        thread
    }

    pub fn mailbox(&self, thread_id: &ThreadId) -> Option<Arc<Mutex<ThreadMailbox>>> {
        self.mailboxes
            .get(thread_id)
            .map(|entry| Arc::clone(entry.value()))
    }

    pub async fn enqueue_user_message(
        &self,
        thread_id: &ThreadId,
        content: String,
        msg_override: Option<MessageOverride>,
    ) -> bool {
        let Some(mailbox) = self.mailbox(thread_id) else {
            return false;
        };
        mailbox
            .lock()
            .await
            .enqueue_user_message(content, msg_override);
        true
    }

    pub async fn enqueue_mailbox_message(
        &self,
        thread_id: &ThreadId,
        message: MailboxMessage,
    ) -> bool {
        let Some(mailbox) = self.mailbox(thread_id) else {
            return false;
        };
        mailbox.lock().await.enqueue_mailbox_message(message);
        true
    }

    pub async fn interrupt_thread(&self, thread_id: &ThreadId) -> bool {
        let Some(mailbox) = self.mailbox(thread_id) else {
            return false;
        };
        mailbox.lock().await.interrupt_stop();
        true
    }

    pub async fn claim_job_result(
        &self,
        thread_id: &ThreadId,
        job_id: &str,
    ) -> Option<MailboxMessage> {
        let mailbox = self.mailbox(thread_id)?;
        let claimed = mailbox.lock().await.claim_job_result(job_id);
        claimed
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
                        if let Some(mailbox) = self.mailboxes.get(entry.key()) {
                            if let Ok(mut mailbox) = mailbox.try_lock() {
                                mailbox.interrupt_stop();
                            }
                        }
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
            self.mailboxes.remove(&thread_id);
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
            self.mailboxes.remove(&thread_id);
        }
        summaries
    }
}
