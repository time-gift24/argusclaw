//! Thread module - multi-turn conversation session management.
//!
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
//! use claw::agents::thread::{Thread, ThreadBuilder, ThreadEvent};
//!
//! // Create a Thread
//! let mut thread = ThreadBuilder::new()
//!     .provider(my_provider)
//!     .build();
//!
//! // Subscribe to events
//! let mut event_rx = thread.subscribe();
//!
//! // Send a message
//! let stream = thread.send_message("Hello!".to_string()).await;
//!
//! // Process events
//! while let Ok(event) = event_rx.recv().await {
//!     match event {
//!         ThreadEvent::TurnCompleted { .. } => break,
//!         _ => {}
//!     }
//! }
//! ```

mod config;
mod error;
#[allow(clippy::module_inception)]
mod thread;
mod types;

pub use config::ThreadConfig;
pub use error::{CompactError, ThreadError};
pub use thread::{Thread, ThreadBuilder};
pub use types::{ThreadInfo, ThreadState};

// Re-export ThreadConfigBuilder for internal use (tests)
#[cfg(test)]
pub use config::ThreadConfigBuilder;
