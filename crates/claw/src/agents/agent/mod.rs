//! Agent runtime module.
//!
//! This module provides:
//! - `Agent`: Manages multiple threads with shared configuration.
//! - `AgentBuilder`: Builder for creating Agent instances.
//! - `AgentHandle`: Handle for accessing a thread through an Agent.
//! - `AgentManager`: Creates and manages runtime Agent instances.

mod manager;
mod runtime;

pub use manager::AgentManager;
pub use runtime::{Agent, AgentBuilder, AgentHandle, AgentRuntimeInfo};
