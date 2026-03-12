//! Agent management module.

pub mod agent;
pub mod compact;
pub mod thread;
pub mod turn;

mod types;

pub use agent::AgentManager;
pub use types::{AgentId, AgentRecord, AgentRepository, AgentRuntimeId, AgentSummary};
