//! Agent run repository trait.

use async_trait::async_trait;

use crate::error::DbError;
use crate::types::{AgentRunId, AgentRunRecord, AgentRunStatus};

/// Repository trait for externally triggered agent run persistence.
#[async_trait]
pub trait AgentRunRepository: Send + Sync {
    /// Insert a new run record.
    async fn insert_agent_run(&self, record: &AgentRunRecord) -> Result<(), DbError>;

    /// Get a run by public run ID.
    async fn get_agent_run(&self, id: &AgentRunId) -> Result<Option<AgentRunRecord>, DbError>;

    /// Update a run lifecycle state and optional terminal payload.
    async fn update_agent_run_status(
        &self,
        id: &AgentRunId,
        status: AgentRunStatus,
        result: Option<&str>,
        error: Option<&str>,
        completed_at: Option<&str>,
        updated_at: &str,
    ) -> Result<(), DbError>;

    /// Delete a run record.
    async fn delete_agent_run(&self, id: &AgentRunId) -> Result<bool, DbError>;
}
