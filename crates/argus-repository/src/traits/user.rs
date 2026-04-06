//! User repository trait for server-side OAuth2 user persistence.

use async_trait::async_trait;

use crate::error::DbError;
use crate::types::{OAuth2Identity, UserRecord};

/// Repository trait for server user persistence.
///
/// Implemented by storage layers (e.g., PostgreSQL) for the server product.
/// Desktop does not use this trait -- it uses `AccountRepository` instead.
#[async_trait]
pub trait UserRepository: Send + Sync {
    /// Upsert a user from OAuth2 identity claims.
    ///
    /// If a user with the given `external_subject` already exists, update
    /// `account` and `display_name`. Otherwise, insert a new row.
    /// Returns the resulting user record.
    async fn upsert_from_oauth2(&self, identity: &OAuth2Identity) -> Result<UserRecord, DbError>;

    /// Get a user by their database-assigned ID.
    async fn get_by_id(&self, id: i64) -> Result<Option<UserRecord>, DbError>;
}
