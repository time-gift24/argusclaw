//! Test utilities for argus LLM and turn testing.
//!
//! Provides mock providers for testing retry behavior, error handling,
//! and other edge cases in CLI applications.

pub mod providers;

pub use providers::{IntermittentFailureProvider, AlwaysFailProvider};
