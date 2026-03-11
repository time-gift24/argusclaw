//! Agent runtime module.
//!
//! This module provides:
//! - `Agent`: Manages multiple threads with shared configuration.
//! - `AgentHandle`: Handle for accessing an Agent.
//! - `RuntimeAgentManager`: Creates and manages Agent instances.

mod runtime;
mod manager;

pub use runtime::{Agent, AgentHandle};
pub use manager::RuntimeAgentManager;
