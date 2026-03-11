//! Agent runtime module.
//!
//! This module provides:
//! - `Agent`: Manages multiple threads with shared configuration.
//! - `AgentHandle`: Handle for accessing an Agent.
//! - `RuntimeAgentManager`: Creates and manages Agent instances.

mod manager;
mod runtime;

pub use manager::RuntimeAgentManager;
pub use runtime::{Agent, AgentHandle};
