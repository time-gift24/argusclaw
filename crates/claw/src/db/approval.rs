//! Approval repository trait for persistence.

use async_trait::async_trait;
use uuid::Uuid;

use argus_repository::DbError;
use crate::protocol::{ApprovalRequest, ApprovalResponse};

/// Repository for persisting approval requests and responses.
#[async_trait]
pub trait ApprovalRepository: Send + Sync {
    /// Insert a new pending request.
    async fn insert_request(&self, request: &ApprovalRequest) -> Result<(), DbError>;

    /// Remove and return a pending request by ID.
    async fn remove_request(&self, id: Uuid) -> Result<Option<ApprovalRequest>, DbError>;

    /// List all pending requests.
    async fn list_pending(&self) -> Result<Vec<ApprovalRequest>, DbError>;

    /// Insert a response (resolved request).
    async fn insert_response(&self, response: &ApprovalResponse) -> Result<(), DbError>;

    /// Clear all pending requests.
    async fn clear_pending(&self) -> Result<usize, DbError>;
}
