//! Argus Repository - Persistence layer for ArgusClaw.
//!
//! This crate provides:
//! - Repository traits for data access abstraction
//! - SQLite implementations of those traits
//! - Domain types for persistence (records, IDs)
//! - Database connection and migration utilities

pub mod error;
pub mod types;
pub mod traits;
pub mod sqlite;

// Re-export main types
pub use error::DbError;
pub use types::*;
pub use traits::*;

// Re-export ArgusSqlite for convenience
pub use sqlite::ArgusSqlite;
