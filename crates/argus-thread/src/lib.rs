//! Argus Thread - multi-turn conversation session management.
//!
//! This crate provides the Thread implementation for managing multi-turn conversations.
//! A Thread represents a long-running conversation session that:
//! - Manages multiple Turns sequentially
//! - Stores message history in memory
//! - Auto-compacts context when approaching token limits
//! - Broadcasts events to subscribers (CLI, Tauri)
//! - Integrates approval flow via hooks
//!
//! # Example
//!
//! ```ignore
//! use argus_thread::{Thread, ThreadBuilder, ThreadConfig};
//! use argus_protocol::llm::ChatMessage;
//!
//! let thread = ThreadBuilder::new()
//!     .provider(my_provider)
//!     .compactor(my_compactor)
//!     .build()
//!     .unwrap();
//!
//! thread.send_message("Hello!".to_string()).await.unwrap();
//! ```

pub mod compact;
pub mod config;
pub mod error;
pub mod thread;
pub mod types;

// Re-export main types
pub use compact::{
    CompactContext, CompactManager, CompactStrategy, Compactor, CompactorManager,
    KeepRecentCompactor, KeepRecentStrategy, KeepTokensCompactor, KeepTokensStrategy,
    LegacyCompactManager, estimate_tokens,
};
pub use config::ThreadConfig;
pub use error::{CompactError, ThreadError};
pub use thread::{Thread, ThreadBuilder};
pub use types::{ThreadInfo, ThreadState};

// Re-export TurnConfig for convenience
pub use argus_turn::TurnConfig;
