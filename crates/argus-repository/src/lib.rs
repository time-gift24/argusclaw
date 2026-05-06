//! Argus Repository - Persistence layer for ArgusClaw.
//!
//! This crate provides:
//! - Repository traits for data access abstraction
//! - SQLite implementations of those traits
//! - Domain types for persistence (records, IDs)
//! - Database connection and migration utilities

pub mod error;
pub mod postgres;
pub mod sqlite;
pub mod traits;
pub mod types;

// Re-export main types
pub use error::DbError;
pub use traits::*;
pub use types::*;

// Re-export ArgusSqlite for convenience
pub use postgres::{ArgusPostgres, connect as connect_postgres, migrate as migrate_postgres};
pub use sqlite::{ArgusSqlite, connect, connect_path, migrate};
