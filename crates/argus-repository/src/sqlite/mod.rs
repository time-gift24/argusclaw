//! SQLite implementations of repository traits.

use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;

use sqlx::Row;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};

use crate::error::DbError;
use argus_llm::{
    Cipher, FileKeySource, HostMacAddressKeySource, KeyMaterialSource, StaticKeySource,
};
use argus_protocol::llm::SecretString;

mod agent;
mod job;
mod llm_provider;
mod thread;
mod workflow;

/// Local result type alias to avoid conflict with argus_protocol::Result.
type DbResult<T> = std::result::Result<T, DbError>;

/// Connect to a SQLite database.
///
/// # Arguments
/// * `database` - Database URL or file path. If it starts with "sqlite:", it's treated as a URL.
pub async fn connect(database: &str) -> DbResult<SqlitePool> {
    let options = if database.starts_with("sqlite:") {
        SqliteConnectOptions::from_str(database).map_err(|e| DbError::ConnectionFailed {
            reason: e.to_string(),
        })?
    } else {
        SqliteConnectOptions::new().filename(database)
    };

    SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options.create_if_missing(true))
        .await
        .map_err(|e| DbError::ConnectionFailed {
            reason: e.to_string(),
        })
}

/// Connect to a SQLite database at a specific path.
pub async fn connect_path(path: &Path) -> DbResult<SqlitePool> {
    let options = SqliteConnectOptions::new()
        .filename(path)
        .create_if_missing(true);

    SqlitePoolOptions::new()
        .max_connections(5)
        .connect_with(options)
        .await
        .map_err(|e| DbError::ConnectionFailed {
            reason: e.to_string(),
        })
}

/// Run database migrations.
pub async fn migrate(pool: &SqlitePool) -> DbResult<()> {
    sqlx::migrate!()
        .run(pool)
        .await
        .map_err(|e| DbError::MigrationFailed {
            reason: e.to_string(),
        })
}

/// Unified SQLite repository implementing all repository traits.
///
/// This struct provides a single entry point for all database operations,
/// sharing a connection pool and encryption keys across all repositories.
pub struct ArgusSqlite {
    pool: SqlitePool,
    write_cipher: Cipher,
    read_ciphers: Vec<Cipher>,
}

impl ArgusSqlite {
    /// Create a new ArgusSqlite with default key sources.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self::with_key_sources(
            pool,
            Arc::new(FileKeySource::from_env_or_default()),
            vec![Arc::new(HostMacAddressKeySource)],
        )
    }

    /// Create an ArgusSqlite with a specific key material.
    #[must_use]
    pub fn new_with_key_material(pool: SqlitePool, key_material: Vec<u8>) -> Self {
        Self::with_key_sources(
            pool,
            Arc::new(StaticKeySource::new(key_material)),
            Vec::new(),
        )
    }

    /// Create an ArgusSqlite with key material and fallbacks.
    #[must_use]
    pub fn new_with_key_material_and_fallbacks(
        pool: SqlitePool,
        key_material: Vec<u8>,
        fallback_key_materials: Vec<Vec<u8>>,
    ) -> Self {
        let fallback_sources: Vec<Arc<dyn KeyMaterialSource>> = fallback_key_materials
            .into_iter()
            .map(|km| Arc::new(StaticKeySource::new(km)) as Arc<dyn KeyMaterialSource>)
            .collect();

        Self::with_key_sources(
            pool,
            Arc::new(StaticKeySource::new(key_material)),
            fallback_sources,
        )
    }

    /// Create an ArgusSqlite with custom key sources.
    #[must_use]
    pub fn with_key_sources(
        pool: SqlitePool,
        key_source: Arc<dyn KeyMaterialSource>,
        fallback_sources: Vec<Arc<dyn KeyMaterialSource>>,
    ) -> Self {
        let mut read_ciphers = vec![Cipher::new_arc(Arc::clone(&key_source))];
        read_ciphers.extend(fallback_sources.into_iter().map(Cipher::new_arc));

        Self {
            pool,
            write_cipher: Cipher::new_arc(key_source),
            read_ciphers,
        }
    }

    /// Get a reference to the underlying pool.
    #[must_use]
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    fn get_column<T>(row: &sqlx::sqlite::SqliteRow, col: &str) -> DbResult<T>
    where
        T: for<'r> sqlx::decode::Decode<'r, sqlx::Sqlite> + sqlx::types::Type<sqlx::Sqlite>,
    {
        row.try_get(col).map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })
    }

    fn decrypt_secret(&self, nonce: &[u8], ciphertext: &[u8]) -> DbResult<SecretString> {
        let mut last_error = None;
        for cipher in &self.read_ciphers {
            match cipher.decrypt(nonce, ciphertext) {
                Ok(secret) => return Ok(secret),
                Err(error) => last_error = Some(error),
            }
        }
        Err(DbError::SecretDecryptionFailed {
            reason: last_error
                .map(|e| e.to_string())
                .unwrap_or_else(|| "no key sources configured".to_string()),
        })
    }
}
