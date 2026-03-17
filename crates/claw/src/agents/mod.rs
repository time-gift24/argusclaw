//! Agent management module.

pub mod agent;
pub(crate) mod builtins;
mod types;

// Re-export from argus-thread and argus-turn crates
pub use argus_thread::{
    self as thread,
    CompactContext, Compactor, CompactorManager,
    KeepRecentCompactor, KeepTokensCompactor,
    Thread, ThreadBuilder, ThreadConfig, ThreadError, ThreadInfo, ThreadState,
};
pub use argus_turn::{
    self as turn,
    execute_turn_streaming, TurnConfig, TurnError, TurnInput, TurnInputBuilder,
    TurnOutput, TurnStreamEvent,
};

#[cfg(feature = "dev")]
pub use agent::Agent;
pub use agent::{AgentBuilder, AgentManager, AgentRuntimeInfo};
pub use types::{AgentId, AgentRecord, AgentRepository};
