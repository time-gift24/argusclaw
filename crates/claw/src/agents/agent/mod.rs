//! Agent runtime module.
//!
//! This module provides:
//! - `Agent`: Manages multiple threads with shared configuration.
//! - `AgentBuilder`: Builder for creating Agent instances.
//! - `AgentManager`: Creates and manages runtime Agent instances.

mod manager;
mod runtime;

pub use manager::AgentManager;
#[cfg(feature = "dev")]
pub use runtime::Agent;
pub use runtime::{AgentBuilder, AgentRuntimeInfo};
