//! UserRepository implementation for PostgreSQL.

use async_trait::async_trait;
use sqlx::Row;

use crate::error::DbError;
use crate::traits::UserRepository;
use crate::types::{OAuth2Identity, UserRecord};

use super::{ArgusPostgres, DbResult};

pub(super) fn get_column<T>(row: &sqlx::postgres::PgRow, col: &str) -> DbResult<T>
where
    T: for<'r> sqlx::decode::Decode<'r, sqlx::Postgres> + sqlx::types::Type<sqlx::Postgres>,
{
    row.try_get(col).map_err(|e| DbError::QueryFailed {
        reason: e.to_string(),
    })
}

#[async_trait]
impl UserRepository for ArgusPostgres {
    async fn upsert_from_oauth2(&self, identity: &OAuth2Identity) -> DbResult<UserRecord> {
        let row = sqlx::query(
            r#"
            INSERT INTO users (external_subject, account, display_name)
            VALUES ($1, $2, $3)
            ON CONFLICT (external_subject) DO UPDATE SET
                account = EXCLUDED.account,
                display_name = EXCLUDED.display_name,
                updated_at = NOW()
            RETURNING id, external_subject, account, display_name
            "#,
        )
        .bind(&identity.external_subject)
        .bind(&identity.account)
        .bind(&identity.display_name)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        map_user_record(&row)
    }

    async fn get_by_id(&self, id: i64) -> DbResult<Option<UserRecord>> {
        let row = sqlx::query(
            "SELECT id, external_subject, account, display_name FROM users WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        row.map(|row| map_user_record(&row)).transpose()
    }
}

fn map_user_record(row: &sqlx::postgres::PgRow) -> DbResult<UserRecord> {
    Ok(UserRecord {
        id: get_column(&row, "id")?,
        external_subject: get_column(&row, "external_subject")?,
        account: get_column(&row, "account")?,
        display_name: get_column(&row, "display_name")?,
    })
}
