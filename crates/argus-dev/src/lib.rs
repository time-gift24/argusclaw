//! Argus Dev - Development and testing tools for ArgusWing.
//!
//! This crate provides utilities for development, testing, and debugging
//! of ArgusWing applications. It includes simplified interfaces for common
//! operations like turn execution, provider testing, and session management.
//!
//! # Example
//!
//! ```ignore
//! use argus_dev::DevTools;
//! use argus_wing::ArgusWing;
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() {
//!     let wing = Arc::new(ArgusWing::init(None).await.unwrap());
//!     let dev_tools = DevTools::init(wing).await.unwrap();
//!
//!     // Execute a turn for testing
//!     // let messages = dev_tools.execute_turn(...).await.unwrap();
//! }
//! ```

pub mod error;
pub mod turn;
pub mod workflow;

pub use error::{DevError, Result};

use crate::turn::execute_turn_with_config;
use crate::Result as DevResult;
use argus_protocol::{
    events::ThreadEvent,
    llm::{ChatMessage, LlmProvider},
};
use argus_turn::TurnConfig;
use argus_wing::ArgusWing;
use std::sync::Arc;
use tokio::sync::broadcast;

/// Development tools for testing and debugging ArgusWing.
///
/// `DevTools` provides a simplified interface for common development
/// operations like executing turns, testing providers, and debugging sessions.
pub struct DevTools {
    wing: Arc<ArgusWing>,
    _event_rx: broadcast::Receiver<ThreadEvent>,
}

impl DevTools {
    /// Initialize a new DevTools instance.
    ///
    /// # Arguments
    ///
    /// * `wing` - The ArgusWing instance to work with
    ///
    /// # Returns
    ///
    /// Returns the initialized DevTools instance wrapped in an Arc.
    ///
    /// # Errors
    ///
    /// Returns an error if initialization fails.
    pub async fn init(wing: Arc<ArgusWing>) -> DevResult<Arc<Self>> {
        let (_event_tx, event_rx) = broadcast::channel(256);
        Ok(Arc::new(Self {
            wing,
            _event_rx: event_rx,
        }))
    }

    /// Get a reference to the ArgusWing instance.
    #[must_use]
    pub fn wing(&self) -> &Arc<ArgusWing> {
        &self.wing
    }

    /// Execute a single turn with the given parameters.
    ///
    /// This is a simplified interface for turn execution in development scenarios.
    ///
    /// # Arguments
    ///
    /// * `provider` - The LLM provider to use
    /// * `messages` - The conversation message history
    /// * `tool_ids` - Optional list of specific tool IDs to use
    ///
    /// # Returns
    ///
    /// Returns the updated message history after turn execution.
    ///
    /// # Errors
    ///
    /// Returns an error if turn execution fails.
    pub async fn execute_turn(
        &self,
        provider: Arc<dyn LlmProvider>,
        messages: Vec<ChatMessage>,
        tool_ids: Option<Vec<String>>,
    ) -> DevResult<Vec<ChatMessage>> {
        let tool_manager = self.wing.tool_manager();
        let output = execute_turn_with_config(
            provider,
            messages,
            tool_manager.clone(),
            tool_ids,
            TurnConfig::default(),
        )
        .await?;

        Ok(output.messages)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dev_tools_type_exists() {
        // This test verifies the DevTools type exists and can be referenced
        let _: &'static str = std::any::type_name::<DevTools>();
    }
}
