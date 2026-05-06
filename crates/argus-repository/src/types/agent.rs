//! Agent persistence types.
//!
//! Re-exported from argus-protocol for backward compatibility.

// Re-export AgentId and AgentRecord from argus-protocol
pub use argus_protocol::{AgentId, AgentRecord};

/// Summary of rows removed by an explicit agent cascade delete.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentDeleteReport {
    pub agent_deleted: bool,
    pub deleted_job_count: u64,
    pub deleted_thread_count: u64,
    pub deleted_session_count: u64,
}

impl AgentDeleteReport {
    #[must_use]
    pub const fn empty(agent_deleted: bool) -> Self {
        Self {
            agent_deleted,
            deleted_job_count: 0,
            deleted_thread_count: 0,
            deleted_session_count: 0,
        }
    }
}
