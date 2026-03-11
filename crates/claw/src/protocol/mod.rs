//! Protocol types shared across modules.
//!
//! This module contains types that need to be shared between multiple modules
//! to avoid circular dependencies (e.g., between `approval` and `tool`).

mod risk_level;

pub use risk_level::RiskLevel;
