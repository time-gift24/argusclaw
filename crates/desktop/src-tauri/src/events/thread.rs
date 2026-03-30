//! Thread event types for frontend communication.
//!
//! Re-exports shared envelope and payload types from `argus_protocol`.
//! These types bridge the internal `ThreadEvent` enum to frontend-consumable
//! JSON payloads.

pub use argus_protocol::{ThreadEventEnvelope, ThreadEventPayload};
