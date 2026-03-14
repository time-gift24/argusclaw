//! Agent management module.

pub mod agent;
pub mod compact;
pub mod thread;
pub mod turn;

mod types;

pub use agent::AgentManager;
pub use compact::{Compactor, CompactorManager};
pub use thread::{Thread, ThreadBuilder, TurnStreamHandle};
pub use turn::{TurnError, TurnInputBuilder, TurnOutput, TurnStreamEvent, execute_turn_streaming};
pub use types::{AgentId, AgentRecord, AgentRepository, AgentRuntimeId, AgentSummary};
