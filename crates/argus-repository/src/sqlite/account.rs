//! AccountRepository implementation for SQLite.

use async_trait::async_trait;
use sqlx::Row;

use argus_protocol::account::{AccountCredentials, AccountRepository};

use crate::error::DbError;
use crate::sqlite::{ArgusSqlite, DbResult};

#[async_trait]
impl AccountRepository for ArgusSqlite {
    async fn has_account(&self) -> argus_protocol::Result<bool> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        Ok(count > 0)
    }

    async fn setup_account(
        &self,
        username: &str,
        ciphertext: &[u8],
        nonce: &[u8],
    ) -> argus_protocol::Result<()> {
        sqlx::query(
            r#"
            INSERT INTO accounts (id, username, password, nonce, created_at, updated_at)
            VALUES (1, ?1, ?2, ?3, datetime('now'), datetime('now'))
            "#,
        )
        .bind(username)
        .bind(ciphertext)
        .bind(nonce)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        Ok(())
    }

    async fn get_credentials(&self) -> argus_protocol::Result<Option<AccountCredentials>> {
        let row = sqlx::query("SELECT username, password, nonce FROM accounts WHERE id = 1")
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        match row {
            Some(r) => Ok(Some(self.map_account_row(&r)?)),
            None => Ok(None),
        }
    }

    async fn get_username(&self) -> argus_protocol::Result<Option<String>> {
        let row = sqlx::query("SELECT username FROM accounts WHERE id = 1")
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        match row {
            Some(r) => Ok(Some(Self::get_account_column::<String>(&r, "username")?)),
            None => Ok(None),
        }
    }
}

impl ArgusSqlite {
    fn get_account_column<T>(row: &sqlx::sqlite::SqliteRow, col: &str) -> DbResult<T>
    where
        T: for<'r> sqlx::decode::Decode<'r, sqlx::Sqlite> + sqlx::types::Type<sqlx::Sqlite>,
    {
        row.try_get(col).map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })
    }

    fn map_account_row(&self, row: &sqlx::sqlite::SqliteRow) -> DbResult<AccountCredentials> {
        Ok(AccountCredentials {
            username: Self::get_account_column(row, "username")?,
            ciphertext: Self::get_account_column(row, "password")?,
            nonce: Self::get_account_column(row, "nonce")?,
        })
    }
}
