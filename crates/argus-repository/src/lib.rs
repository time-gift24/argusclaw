//! Argus Repository - Persistence layer for ArgusClaw.
//!
//! This crate provides:
//! - Repository traits for data access abstraction
//! - SQLite implementations of those traits (desktop)
//! - PostgreSQL implementations of those traits (server)
//! - Domain types for persistence (records, IDs)
//! - Database connection and migration utilities

pub mod error;
pub mod sqlite;
pub mod traits;
pub mod types;

#[cfg(feature = "postgres")]
pub mod postgres;

// Re-export main types
pub use error::DbError;
pub use traits::*;
pub use types::*;

// Re-export ArgusSqlite for convenience
pub use sqlite::{ArgusSqlite, connect, connect_path, migrate};
