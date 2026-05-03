//! User repository trait for trusted-header chat ownership mapping.

use argus_protocol::UserId;
use async_trait::async_trait;

use crate::error::DbError;

/// Maps an external trusted-header identity to an internal persisted user ID.
#[async_trait]
pub trait UserRepository: Send + Sync {
    async fn resolve_user(
        &self,
        external_id: &str,
        display_name: Option<&str>,
    ) -> Result<UserId, DbError>;
}
