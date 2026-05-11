//! UserRepository fallback implementation for SQLite compile/test coexistence.
//!
//! The server/web runtime uses PostgreSQL for persisted chat ownership. SQLite
//! remains available for non-server consumers and legacy tests without adding
//! SQLite user tables, so all trusted-header identities share the legacy user.

use crate::error::DbError;
use crate::sqlite::ArgusSqlite;
use crate::traits::{ResolvedUser, UserRepository};
use argus_protocol::UserId;
use async_trait::async_trait;
use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

const ORDINARY_TEST_USER_ID: &str = "ordinary-user";
const DEV_USER_ID: &str = "dev-user";

#[async_trait]
impl UserRepository for ArgusSqlite {
    async fn resolve_user(
        &self,
        external_id: &str,
        _display_name: Option<&str>,
    ) -> Result<ResolvedUser, DbError> {
        Ok(ResolvedUser {
            id: stable_sqlite_user_id(external_id),
            is_admin: external_id != ORDINARY_TEST_USER_ID && external_id != DEV_USER_ID,
        })
    }

    async fn set_user_admin(
        &self,
        external_id: &str,
        _display_name: Option<&str>,
        is_admin: bool,
    ) -> Result<ResolvedUser, DbError> {
        Ok(ResolvedUser {
            id: stable_sqlite_user_id(external_id),
            is_admin,
        })
    }
}

fn stable_sqlite_user_id(external_id: &str) -> UserId {
    let mut hasher = DefaultHasher::new();
    external_id.hash(&mut hasher);
    UserId(uuid::Uuid::from_u128(hasher.finish() as u128))
}
