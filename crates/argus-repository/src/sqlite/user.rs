//! UserRepository fallback implementation for SQLite compile/test coexistence.
//!
//! The server/web runtime uses PostgreSQL for persisted chat ownership. SQLite
//! remains available for non-server consumers and legacy tests without adding
//! SQLite user tables, so all trusted-header identities share the legacy user.

use crate::error::DbError;
use crate::sqlite::ArgusSqlite;
use crate::traits::UserRepository;
use argus_protocol::UserId;
use async_trait::async_trait;

#[async_trait]
impl UserRepository for ArgusSqlite {
    async fn resolve_user(
        &self,
        _external_id: &str,
        _display_name: Option<&str>,
    ) -> Result<UserId, DbError> {
        Ok(UserId::default())
    }
}
