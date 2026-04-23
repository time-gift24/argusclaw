use async_trait::async_trait;

use crate::error::DbError;
use crate::sqlite::ArgusSqlite;
use crate::traits::AdminSettingsRepository;
use crate::types::AdminSettingsRecord;

#[async_trait]
impl AdminSettingsRepository for ArgusSqlite {
    async fn get_admin_settings(&self) -> Result<AdminSettingsRecord, DbError> {
        let record = sqlx::query_as::<_, AdminSettingsRow>(
            "SELECT instance_name FROM admin_settings WHERE id = 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|error| DbError::QueryFailed {
            reason: error.to_string(),
        })?
        .map(Into::into)
        .unwrap_or_default();

        Ok(record)
    }

    async fn upsert_admin_settings(
        &self,
        record: &AdminSettingsRecord,
    ) -> Result<AdminSettingsRecord, DbError> {
        sqlx::query(
            "INSERT INTO admin_settings (id, instance_name)
             VALUES (1, ?1)
             ON CONFLICT(id) DO UPDATE SET
                instance_name = excluded.instance_name,
                updated_at = CURRENT_TIMESTAMP",
        )
        .bind(&record.instance_name)
        .execute(&self.pool)
        .await
        .map_err(|error| DbError::QueryFailed {
            reason: error.to_string(),
        })?;

        self.get_admin_settings().await
    }
}

#[derive(sqlx::FromRow)]
struct AdminSettingsRow {
    instance_name: String,
}

impl From<AdminSettingsRow> for AdminSettingsRecord {
    fn from(value: AdminSettingsRow) -> Self {
        Self {
            instance_name: value.instance_name,
        }
    }
}
