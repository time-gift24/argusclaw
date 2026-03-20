//! Execution approval manager — gates dangerous operations behind human approval.

//!
//! This module provides the runtime approval manager that:
//! - Stores pending approval requests
//! - Tracks per-agent limits
//! - Provides event broadcasting
//! - Handles request resolution

use chrono::Utc;
use dashmap::DashMap;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing::{debug, info, warn};
use uuid::Uuid;

use super::error::ApprovalError;
use super::policy::ApprovalPolicy;
use argus_protocol::{ApprovalDecision, ApprovalEvent, ApprovalRequest, ApprovalResponse};

/// Maximum pending requests per agent.
pub const MAX_PENDING_PER_AGENT: usize = 5;
/// Manages approval requests with oneshot channels for blocking resolution.
///
/// The manager is designed to be thread-safe and can be shared across multiple
/// agents or components.
pub struct ApprovalManager {
    pending: DashMap<Uuid, PendingRequest>,
    policy: std::sync::RwLock<ApprovalPolicy>,
    /// Broadcast sender for approval events (RequestCreated, Resolved).
    event_tx: broadcast::Sender<ApprovalEvent>,
}
struct PendingRequest {
    request: ApprovalRequest,
    sender: tokio::sync::oneshot::Sender<ApprovalDecision>,
}
impl ApprovalManager {
    /// Create a new approval manager with the given policy.
    pub fn new(policy: ApprovalPolicy) -> Self {
        let (event_tx, _) = broadcast::channel(16);
        Self {
            pending: DashMap::new(),
            policy: std::sync::RwLock::new(policy),
            event_tx,
        }
    }
    /// Create a new approval manager wrapped in Arc.
    pub fn new_shared(policy: ApprovalPolicy) -> Arc<Self> {
        Arc::new(Self::new(policy))
    }
    /// Subscribe to approval events (RequestCreated, Resolved).
    ///
    /// The returned receiver will get all events broadcast after subscription.
    /// Old messages are automatically dropped if the subscriber is slow.
    pub fn subscribe(&self) -> broadcast::Receiver<ApprovalEvent> {
        self.event_tx.subscribe()
    }
    /// Check if a tool requires approval based on current policy.
    pub fn requires_approval(&self, tool_name: &str) -> bool {
        let policy = self.policy.read().unwrap_or_else(|e| e.into_inner());
        policy.requires_approval(tool_name)
    }
    /// Submit an approval request. Returns a future that resolves when approved/denied/timed out.
    ///
    /// # Errors
    ///
    /// Returns `ApprovalDecision::Denied` if the agent has too many pending requests.
    pub async fn request_approval(&self, req: ApprovalRequest) -> ApprovalDecision {
        // Check per-agent pending limit
        let agent_pending = self
            .pending
            .iter()
            .filter(|r| r.value().request.agent_id == req.agent_id)
            .count();
        if agent_pending >= MAX_PENDING_PER_AGENT {
            warn!(
                agent_id = %req.agent_id,
                pending_count = agent_pending,
                max = MAX_PENDING_PER_AGENT,
                "Approval request rejected: too many pending"
            );
            return ApprovalDecision::Denied;
        }
        let timeout = std::time::Duration::from_secs(req.timeout_secs);
        let id = req.id;
        let (tx, rx) = tokio::sync::oneshot::channel();
        self.pending.insert(
            id,
            PendingRequest {
                request: req.clone(),
                sender: tx,
            },
        );
        // Broadcast RequestCreated event (ignore if no subscribers)
        let _ = self.event_tx.send(ApprovalEvent::RequestCreated(req));
        info!(request_id = %id, "Approval request submitted, waiting for resolution");
        match tokio::time::timeout(timeout, rx).await {
            Ok(Ok(decision)) => {
                debug!(request_id = %id, ?decision, "Approval resolved");
                decision
            }
            _ => {
                self.pending.remove(&id);
                warn!(request_id = %id, "Approval request timed out");
                ApprovalDecision::TimedOut
            }
        }
    }
    /// Resolve a pending request (called by API/UI).
    ///
    /// # Errors
    ///
    /// - `ApprovalError::NotFound` if no pending request exists with the given ID.
    pub fn resolve(
        &self,
        request_id: Uuid,
        decision: ApprovalDecision,
        decided_by: Option<String>,
    ) -> Result<ApprovalResponse, ApprovalError> {
        match self.pending.remove(&request_id) {
            Some((_, pending)) => {
                let response = ApprovalResponse {
                    request_id,
                    decision,
                    decided_at: Utc::now(),
                    decided_by,
                };
                // Send decision to waiting agent (ignore error if receiver dropped)
                let _ = pending.sender.send(decision);
                // Broadcast Resolved event (ignore if no subscribers)
                let _ = self
                    .event_tx
                    .send(ApprovalEvent::Resolved(response.clone()));
                info!(request_id = %request_id, ?decision, "Approval request resolved");
                Ok(response)
            }
            None => Err(ApprovalError::NotFound(request_id)),
        }
    }
    /// List all pending requests (for API/dashboard display).
    pub fn list_pending(&self) -> Vec<ApprovalRequest> {
        self.pending
            .iter()
            .map(|r| r.value().request.clone())
            .collect()
    }
    /// Number of pending requests.
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }
    /// Update the approval policy (for hot-reload).
    pub fn update_policy(&self, policy: ApprovalPolicy) {
        *self.policy.write().unwrap_or_else(|e| e.into_inner()) = policy;
    }
    /// Get a copy of the current policy.
    pub fn policy(&self) -> ApprovalPolicy {
        self.policy
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}
// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------
#[cfg(test)]
mod tests {
    use super::*;
    use argus_protocol::RiskLevel;
    fn default_manager() -> ApprovalManager {
        ApprovalManager::new(ApprovalPolicy::default())
    }
    fn make_request(agent_id: &str, tool_name: &str, timeout_secs: u64) -> ApprovalRequest {
        ApprovalRequest::new(
            agent_id.to_string(),
            tool_name.to_string(),
            "test action".to_string(),
            timeout_secs,
            RiskLevel::Critical,
        )
    }
    #[test]
    fn test_requires_approval_default() {
        let mgr = default_manager();
        assert!(mgr.requires_approval("shell"));
        assert!(!mgr.requires_approval("file_read"));
        assert!(mgr.requires_approval("http"));
    }
    #[test]
    fn test_requires_approval_custom_policy() {
        let policy = ApprovalPolicy {
            require_approval: vec!["file_write".to_string(), "file_delete".to_string()],
            timeout_secs: 30,
            auto_approve_autonomous: false,
            auto_approve: false,
        };
        let mgr = ApprovalManager::new(policy);
        assert!(mgr.requires_approval("file_write"));
        assert!(mgr.requires_approval("file_delete"));
        assert!(!mgr.requires_approval("shell"));
        assert!(!mgr.requires_approval("http"));
        assert!(!mgr.requires_approval("file_read"));
    }
    #[test]
    fn test_resolve_nonexistent() {
        let mgr = default_manager();
        let result = mgr.resolve(Uuid::new_v4(), ApprovalDecision::Approved, None);
        assert!(result.is_err());
        match result.unwrap_err() {
            ApprovalError::NotFound(_) => {}
            other => panic!("expected NotFound error, got {other:?}"),
        }
    }
    #[test]
    fn test_list_pending_empty() {
        let mgr = default_manager();
        assert!(mgr.list_pending().is_empty());
    }
    #[test]
    fn test_update_policy() {
        let mgr = default_manager();
        assert!(mgr.requires_approval("shell"));
        assert!(mgr.requires_approval("http"));
        assert!(!mgr.requires_approval("file_write"));
        let new_policy = ApprovalPolicy {
            require_approval: vec!["file_write".to_string()],
            timeout_secs: 120,
            auto_approve_autonomous: true,
            auto_approve: false,
        };
        mgr.update_policy(new_policy);
        assert!(!mgr.requires_approval("shell"));
        assert!(!mgr.requires_approval("http"));
        assert!(mgr.requires_approval("file_write"));
        let policy = mgr.policy();
        assert_eq!(policy.timeout_secs, 120);
        assert!(policy.auto_approve_autonomous);
    }
    #[test]
    fn test_pending_count() {
        let mgr = default_manager();
        assert_eq!(mgr.pending_count(), 0);
    }
    #[tokio::test]
    async fn test_request_approval_timeout() {
        let mgr = Arc::new(default_manager());
        let req = make_request("agent-1", "shell", 10);
        let decision = mgr.request_approval(req).await;
        assert_eq!(decision, ApprovalDecision::TimedOut);
        // After timeout, pending map should be cleaned up
        assert_eq!(mgr.pending_count(), 0);
    }
    #[tokio::test]
    async fn test_request_approval_approve() {
        let mgr = Arc::new(default_manager());
        let req = make_request("agent-1", "shell", 60);
        let request_id = req.id;
        let mgr2 = Arc::clone(&mgr);
        tokio::spawn(async move {
            // Small delay to let the request register
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            let result = mgr2.resolve(
                request_id,
                ApprovalDecision::Approved,
                Some("admin".to_string()),
            );
            assert!(result.is_ok());
            let resp = result.unwrap();
            assert_eq!(resp.decision, ApprovalDecision::Approved);
            assert_eq!(resp.decided_by, Some("admin".to_string()));
        });
        let decision = mgr.request_approval(req).await;
        assert_eq!(decision, ApprovalDecision::Approved);
    }
    #[tokio::test]
    async fn test_request_approval_deny() {
        let mgr = Arc::new(default_manager());
        let req = make_request("agent-1", "shell", 60);
        let request_id = req.id;
        let mgr2 = Arc::clone(&mgr);
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            let result = mgr2.resolve(request_id, ApprovalDecision::Denied, None);
            assert!(result.is_ok());
        });
        let decision = mgr.request_approval(req).await;
        assert_eq!(decision, ApprovalDecision::Denied);
    }
    #[tokio::test]
    async fn test_max_pending_per_agent() {
        let mgr = Arc::new(default_manager());
        // Fill up 5 pending requests for agent-1 (they will all be waiting)
        let mut ids = Vec::new();
        for _ in 0..MAX_PENDING_PER_AGENT {
            let req = make_request("agent-1", "shell", 300);
            ids.push(req.id);
            let mgr_clone = Arc::clone(&mgr);
            tokio::spawn(async move {
                mgr_clone.request_approval(req).await;
            });
        }
        // Give spawned tasks time to register
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(mgr.pending_count(), MAX_PENDING_PER_AGENT);
        // 6th request for the same agent should be immediately denied
        let req6 = make_request("agent-1", "shell", 300);
        let decision = mgr.request_approval(req6).await;
        assert_eq!(decision, ApprovalDecision::Denied);
        // A different agent should still be able to submit
        let req_other = make_request("agent-2", "shell", 300);
        let other_id = req_other.id;
        let mgr2 = Arc::clone(&mgr);
        tokio::spawn(async move {
            mgr2.request_approval(req_other).await;
        });
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        assert_eq!(mgr.pending_count(), MAX_PENDING_PER_AGENT + 1);
        // Cleanup: resolve all pending to avoid hanging tasks
        for id in &ids {
            let _ = mgr.resolve(*id, ApprovalDecision::Denied, None);
        }
        let _ = mgr.resolve(other_id, ApprovalDecision::Denied, None);
    }
    #[test]
    fn test_policy_defaults() {
        let mgr = default_manager();
        let policy = mgr.policy();
        assert_eq!(policy.require_approval, vec!["shell".to_string(), "http".to_string()]);
        assert_eq!(policy.timeout_secs, 60);
        assert!(!policy.auto_approve_autonomous);
    }
    #[tokio::test]
    async fn test_subscribe_request_created() {
        let mgr = Arc::new(default_manager());
        let mut rx = mgr.subscribe();
        let req = make_request("agent-1", "shell", 60);
        let request_id = req.id;
        let mgr2 = Arc::clone(&mgr);
        // Spawn task to resolve after a short delay
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            let _ = mgr2.resolve(request_id, ApprovalDecision::Approved, None);
        });
        let _ = mgr.request_approval(req).await;
        // Should receive RequestCreated event
        let event = rx.try_recv().expect("should receive RequestCreated event");
        match event {
            ApprovalEvent::RequestCreated(r) => {
                assert_eq!(r.id, request_id);
                assert_eq!(r.agent_id, "agent-1");
                assert_eq!(r.tool_name, "shell");
            }
            ApprovalEvent::Resolved(_) => panic!("expected RequestCreated, got Resolved"),
        }
    }
    #[tokio::test]
    async fn test_subscribe_resolved() {
        let mgr = Arc::new(default_manager());
        let mut rx = mgr.subscribe();
        let req = make_request("agent-1", "shell", 60);
        let request_id = req.id;
        let mgr2 = Arc::clone(&mgr);
        // Spawn task to resolve after a short delay
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            let _ = mgr2.resolve(
                request_id,
                ApprovalDecision::Approved,
                Some("admin".to_string()),
            );
        });
        let _ = mgr.request_approval(req).await;
        // Skip RequestCreated event
        let _ = rx.try_recv();
        // Should receive Resolved event
        let event = rx.try_recv().expect("should receive Resolved event");
        match event {
            ApprovalEvent::RequestCreated(_) => panic!("expected Resolved, got RequestCreated"),
            ApprovalEvent::Resolved(resp) => {
                assert_eq!(resp.request_id, request_id);
                assert_eq!(resp.decision, ApprovalDecision::Approved);
                assert_eq!(resp.decided_by, Some("admin".to_string()));
            }
        }
    }
    #[tokio::test]
    async fn test_multiple_subscribers() {
        let mgr = Arc::new(default_manager());
        let mut rx1 = mgr.subscribe();
        let mut rx2 = mgr.subscribe();
        let req = make_request("agent-1", "shell", 60);
        let request_id = req.id;
        let mgr2 = Arc::clone(&mgr);
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            let _ = mgr2.resolve(request_id, ApprovalDecision::Denied, None);
        });
        let _ = mgr.request_approval(req).await;
        // Both subscribers should receive events
        assert!(rx1.try_recv().is_ok());
        assert!(rx2.try_recv().is_ok());
    }
    #[tokio::test]
    async fn test_no_subscribers_still_works() {
        let mgr = Arc::new(default_manager());
        // No subscription created
        let req = make_request("agent-1", "shell", 60);
        let request_id = req.id;
        let mgr2 = Arc::clone(&mgr);
        tokio::spawn(async move {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            let _ = mgr2.resolve(request_id, ApprovalDecision::Approved, None);
        });
        // Should still work without subscribers
        let decision = mgr.request_approval(req).await;
        assert_eq!(decision, ApprovalDecision::Approved);
    }
}
