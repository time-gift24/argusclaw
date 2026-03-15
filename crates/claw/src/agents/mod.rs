//! Agent management module.

pub mod agent;

// Internal modules: pub for dev feature, otherwise crate-internal
#[cfg(feature = "dev")]
pub mod compact;
#[cfg(not(feature = "dev"))]
pub(crate) mod compact;

#[cfg(feature = "dev")]
pub mod thread;
#[cfg(not(feature = "dev"))]
pub(crate) mod thread;

#[cfg(feature = "dev")]
pub mod turn;
#[cfg(not(feature = "dev"))]
pub(crate) mod turn;

mod types;

pub use agent::{Agent, AgentBuilder, AgentHandle, AgentManager, AgentRuntimeInfo};
pub use types::{AgentId, AgentRecord, AgentRepository, AgentRuntimeId, AgentSummary};

// Re-export thread types still needed by external consumers
pub use thread::{ThreadConfig, ThreadConfigBuilder, ThreadInfo, ThreadState, TurnStreamHandle};
