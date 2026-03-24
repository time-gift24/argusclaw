//! SSE broadcaster for job events.
//!
//! Provides session-scoped broadcast channel for job completion events.

use tokio::sync::broadcast;

use crate::types::JobStatusEvent;

/// SSE broadcaster for job events.
///
/// Each session has its own broadcast channel. Job completion events
/// are broadcast to all subscribers of the session.
#[derive(Debug, Clone)]
pub struct SseBroadcaster {
    tx: broadcast::Sender<JobStatusEvent>,
}

impl SseBroadcaster {
    /// Create a new SSE broadcaster.
    pub fn new() -> Self {
        let (tx, _) = broadcast::channel(100);
        Self { tx }
    }

    /// Subscribe to job events.
    pub fn subscribe(&self) -> broadcast::Receiver<JobStatusEvent> {
        self.tx.subscribe()
    }

    /// Broadcast a job status event.
    pub fn broadcast(&self, event: JobStatusEvent) {
        if let Err(e) = self.tx.send(event) {
            tracing::warn!("failed to broadcast job event: {}", e);
        }
    }

    /// Broadcast job completed event.
    pub fn broadcast_completed(&self, job_id: String, session_id: Option<String>) {
        self.broadcast(JobStatusEvent {
            job_id,
            status: "completed".to_string(),
            session_id,
            message: Some("Job completed successfully".to_string()),
        });
    }

    /// Broadcast job failed event.
    pub fn broadcast_failed(&self, job_id: String, session_id: Option<String>, message: String) {
        self.broadcast(JobStatusEvent {
            job_id,
            status: "failed".to_string(),
            session_id,
            message: Some(message.clone()),
        });
    }

    /// Broadcast job stuck event.
    pub fn broadcast_stuck(&self, job_id: String, session_id: Option<String>, message: String) {
        self.broadcast(JobStatusEvent {
            job_id,
            status: "stuck".to_string(),
            session_id,
            message: Some(message.clone()),
        });
    }
}

impl Default for SseBroadcaster {
    fn default() -> Self {
        Self::new()
    }
}
