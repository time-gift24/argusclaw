//! Argus Agent - thread-owned turn execution and conversation management.
//!
//! The public entry point is [`Thread`], which owns agent configuration,
//! compaction, tool resolution, and turn settlement. Turn execution remains an
//! internal implementation detail of the thread runtime.
//!
//! # Example
//!
//! ```ignore
//! use std::sync::Arc;
//!
//! use argus_agent::{LlmThreadCompactor, ThreadBuilder, TurnCancellation};
//! use argus_protocol::{AgentRecord, SessionId};
//!
//! let provider = my_provider;
//! let mut thread = ThreadBuilder::new()
//!     .provider(Arc::clone(&provider))
//!     .compactor(Arc::new(LlmThreadCompactor::new(provider)))
//!     .agent_record(Arc::new(AgentRecord::default()))
//!     .session_id(SessionId::new())
//!     .build()?;
//!
//! let record = thread
//!     .execute_turn("Hello!".to_string(), None, TurnCancellation::new())
//!     .await?;
//! ```

pub mod compact;
pub mod config;
pub mod error;
pub mod history;
pub mod plan_hook;
pub mod plan_store;
pub mod plan_tool;
pub mod thread;
pub mod thread_bootstrap;
pub mod thread_trace_store;
pub mod turn_log_store;

pub mod tool_context;
pub mod trace;
mod turn;
pub mod types;

// ---------------------------------------------------------------------------
// Public exports
// ---------------------------------------------------------------------------

// Turn execution config
pub use config::{TurnConfig, TurnConfigBuilder, TurnStreamEvent};
pub use error::{TurnError, TurnLogError};
pub use history::{TurnRecord, TurnRecordKind};
pub use trace::TraceConfig;
pub use turn::TurnCancellation;

// Thread (high-level)
pub use compact::thread::LlmThreadCompactor;
pub use compact::turn::LlmTurnCompactor;
pub use compact::{CompactResult, Compactor};
pub use config::ThreadConfig;
pub use error::{CompactError, ThreadError};
pub use plan_store::FilePlanStore;
pub use thread::{Thread, ThreadBuilder};
pub use types::{ThreadInfo, ThreadState};

// Re-export hook types from argus-protocol for convenience
pub use argus_protocol::{HookAction, HookEvent, HookHandler, HookRegistry, ToolHookContext};
