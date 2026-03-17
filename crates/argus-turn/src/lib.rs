//! Argus Turn - single turn execution with LLM and tool support.
//!
//! This crate provides the core turn execution logic for agent-based conversations.
//! A "Turn" represents a single execution cycle: LLM → Tool → LLM (with parallel tool support).
//!
//! # Example
//!
//! ```ignore
//! use argus_turn::{TurnInput, TurnInputBuilder, TurnOutput, execute_turn};
//! use argus_protocol::llm::ChatMessage;
//!
//! let input = TurnInputBuilder::new()
//!     .provider(my_provider)
//!     .messages(vec![ChatMessage::user("Hello!")])
//!     .build()
//!     .unwrap();
//!
//! let output = execute_turn(input, TurnConfig::default()).await.unwrap();
//! ```

pub mod config;
pub mod error;
pub mod execution;

pub use config::{TurnConfig, TurnConfigBuilder, TurnInput, TurnInputBuilder, TurnOutput, TurnOutputBuilder, TurnStreamEvent};
pub use error::TurnError;
pub use execution::{execute_turn, execute_turn_streaming, ExecutionMode};

// Re-export hook types from argus-protocol for convenience
pub use argus_protocol::{
    BeforeCallLLMContext, BeforeCallLLMResult, HookAction, HookEvent, HookHandler, HookRegistry,
    ToolHookContext,
};
