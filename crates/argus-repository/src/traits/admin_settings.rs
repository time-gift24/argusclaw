use async_trait::async_trait;

use crate::error::DbError;
use crate::types::AdminSettingsRecord;

#[async_trait]
pub trait AdminSettingsRepository: Send + Sync {
    async fn get_admin_settings(&self) -> Result<AdminSettingsRecord, DbError>;

    async fn upsert_admin_settings(
        &self,
        record: &AdminSettingsRecord,
    ) -> Result<AdminSettingsRecord, DbError>;
}
