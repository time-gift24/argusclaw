//! PostgreSQL implementations of repository traits for the server product.
//!
//! Provides multi-user isolation, OAuth2 user persistence, and owner-aware
//! session/thread/job queries alongside the existing SQLite desktop product.

mod agent;
mod llm_provider;
mod provider_token_credential;
mod session;
mod thread;
mod user;

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

use crate::error::DbError;

/// Local result type alias.
type DbResult<T> = std::result::Result<T, DbError>;

/// PostgreSQL schema DDL for server tables.
///
/// This creates the `users`, `provider_token_credentials` tables, and adds
/// `owner_user_id` to `sessions` and `is_enabled` to `agents`. These are
/// additive columns that coexist with the desktop SQLite schema replicated
/// to PostgreSQL.
const SCHEMA_DDL: &str = include_str!("schema.sql");

/// Run the PostgreSQL schema migration (idempotent).
pub async fn migrate(pool: &PgPool) -> DbResult<()> {
    sqlx::query(SCHEMA_DDL)
        .execute(pool)
        .await
        .map_err(|e| DbError::MigrationFailed {
            reason: e.to_string(),
        })?;
    Ok(())
}

/// Connect to a PostgreSQL database.
pub async fn connect(database_url: &str) -> DbResult<PgPool> {
    PgPoolOptions::new()
        .max_connections(10)
        .connect(database_url)
        .await
        .map_err(|e| DbError::ConnectionFailed {
            reason: e.to_string(),
        })
}

/// Unified PostgreSQL repository implementing server-side repository traits.
///
/// Unlike `ArgusSqlite`, this does not need encryption keys because
/// provider token credentials are stored with application-level encryption
/// before being persisted.
pub struct ArgusPostgres {
    pool: PgPool,
}

impl ArgusPostgres {
    /// Create a new ArgusPostgres from an existing pool.
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Get a reference to the underlying pool.
    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }
}
