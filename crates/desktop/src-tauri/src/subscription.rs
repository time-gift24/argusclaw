//! Thread subscription management for event forwarding.
//!
//! This module manages active thread subscriptions and forwards events
//! from ArgusWing to the frontend via Tauri events.

use std::collections::HashMap;
use std::sync::Arc;

use argus_protocol::{SessionId, ThreadEvent, ThreadId};
use argus_wing::ArgusWing;
use tauri::{AppHandle, Emitter};
use tokio::sync::{broadcast, Mutex};
use tokio_util::sync::CancellationToken;
use tracing::{debug, error, warn};

use crate::events::ThreadEventEnvelope;

/// Thread subscription state shared across handlers.
#[derive(Default)]
pub struct ThreadSubscriptions {
    inner: Mutex<SubscriptionsInner>,
}

#[derive(Default)]
struct SubscriptionsInner {
    subscriptions: HashMap<String, CancellationToken>,
}

impl ThreadSubscriptions {
    /// Create a new subscription manager.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Start a forwarder for a thread.
    ///
    /// If a forwarder already exists for this session key, it is cancelled first.
    pub async fn start_forwarder(
        &self,
        session_key: String,
        session_id: SessionId,
        thread_id: ThreadId,
        app: AppHandle,
        wing: Arc<ArgusWing>,
    ) -> Result<(), String> {
        let mut inner = self.inner.lock().await;

        // Cancel existing subscription if any
        if let Some(token) = inner.subscriptions.remove(&session_key) {
            token.cancel();
        }

        // Subscribe to thread events
        let receiver = wing
            .subscribe(session_id, thread_id)
            .await
            .ok_or_else(|| "Thread not found".to_string())?;

        let token = CancellationToken::new();
        let cancellation_token = token.clone();
        inner.subscriptions.insert(session_key.clone(), token);

        let session_id_str = session_id.inner().to_string();

        // Spawn the forwarder task
        tokio::spawn(async move {
            Self::forward_events(
                receiver,
                session_id_str,
                app,
                cancellation_token,
                session_key,
            )
            .await;
        });

        Ok(())
    }

    /// Stop a forwarder for a session.
    #[allow(dead_code)]
    pub async fn stop(&self, session_key: &str) {
        let mut inner = self.inner.lock().await;
        if let Some(token) = inner.subscriptions.remove(session_key) {
            token.cancel();
        }
    }

    /// Stop all forwarders.
    #[allow(dead_code)]
    pub async fn stop_all(&self) {
        let mut inner = self.inner.lock().await;
        for token in inner.subscriptions.values() {
            token.cancel();
        }
        inner.subscriptions.clear();
    }

    async fn forward_events(
        mut receiver: broadcast::Receiver<ThreadEvent>,
        session_id: String,
        app: AppHandle,
        cancellation_token: CancellationToken,
        session_key: String,
    ) {
        loop {
            tokio::select! {
                _ = cancellation_token.cancelled() => {
                    debug!("Subscription cancelled for session: {}", session_key);
                    break;
                }
                result = receiver.recv() => {
                    match result {
                        Ok(event) => {
                            if let Some(envelope) = ThreadEventEnvelope::from_thread_event(
                                session_id.clone(),
                                event,
                            ) {
                                if let Err(e) = app.emit("thread:event", &envelope) {
                                    error!("Failed to emit thread event: {}", e);
                                }
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => {
                            warn!("Thread event channel closed for session: {}", session_key);
                            break;
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!(
                                "Thread event channel lagged by {} messages for session: {}",
                                n, session_key
                            );
                            // Continue receiving - the frontend will request a snapshot refresh
                        }
                    }
                }
            }
        }
    }
}
