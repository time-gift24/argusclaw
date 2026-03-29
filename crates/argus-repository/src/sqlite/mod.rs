//! SQLite implementations of repository traits.

use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;

use sqlx::Row;
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};

use crate::error::DbError;
use argus_crypto::{Cipher, FileKeySource, KeyMaterialSource, StaticKeySource};
use argus_protocol::llm::SecretString;

mod account;
mod agent;
mod job;
mod llm_provider;
mod session;
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
            vec![Arc::new(FileKeySource::from_env_or_default())],
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

    /// Repair legacy placeholder agent IDs that were incorrectly persisted as `0`.
    ///
    /// This is a one-time migration that:
    /// 1. Reads all agents with `id = 0`
    /// 2. Deletes the placeholder rows
    /// 3. Re-inserts each with auto-generated real IDs
    /// 4. Updates foreign keys in `threads` and `jobs` tables
    pub async fn repair_placeholder_ids(&self) -> DbResult<()> {
        #[derive(sqlx::FromRow)]
        struct AgentRow {
            display_name: String,
            description: String,
            version: String,
            provider_id: Option<i64>,
            model_id: Option<String>,
            system_prompt: String,
            tool_names: String,
            max_tokens: Option<i64>,
            temperature: Option<i64>,
            thinking_config: Option<String>,
        }

        let placeholder_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM agents WHERE id = 0")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        if placeholder_count == 0 {
            return Ok(());
        }

        // Read all placeholder rows into memory first
        let placeholder: AgentRow = sqlx::query_as(
            "SELECT display_name, description, version, provider_id, model_id, system_prompt,
                    tool_names, max_tokens, temperature, thinking_config
             FROM agents WHERE id = 0",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: format!("failed to fetch placeholder: {}", e),
        })?;

        let AgentRow {
            display_name,
            description,
            version,
            provider_id,
            model_id,
            system_prompt,
            tool_names,
            max_tokens,
            temperature,
            thinking_config,
        } = placeholder;

        let mut tx = self.pool.begin().await.map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        // Delete placeholder row first (no conflict possible now)
        sqlx::query("DELETE FROM agents WHERE id = 0")
            .execute(&mut *tx)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;

        // Re-insert with auto-generated id; ON CONFLICT: if name exists, do nothing
        sqlx::query(
            "INSERT INTO agents (display_name, description, version, provider_id, model_id,
                                 system_prompt, tool_names, max_tokens, temperature, thinking_config)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(display_name) DO NOTHING",
        )
        .bind(&display_name)
        .bind(&description)
        .bind(&version)
        .bind(provider_id)
        .bind(&model_id)
        .bind(&system_prompt)
        .bind(&tool_names)
        .bind(max_tokens)
        .bind(temperature)
        .bind(&thinking_config)
        .execute(&mut *tx)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        // Get the new id: last_insert_rowid if inserted, otherwise find by display_name
        let last_id: i64 = sqlx::query_scalar("SELECT last_insert_rowid()")
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: format!("failed to get last_insert_rowid: {}", e),
            })?;

        let repaired_id = if last_id == 0 {
            // ON CONFLICT fired: name already exists, find it
            sqlx::query_scalar("SELECT id FROM agents WHERE display_name = ? AND id != 0")
                .bind(&display_name)
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| DbError::QueryFailed {
                    reason: format!("failed to get existing id for '{}': {}", display_name, e),
                })?
        } else {
            last_id
        };

        // Update foreign keys in threads and jobs tables
        for statement in [
            "UPDATE threads SET template_id = ? WHERE template_id = 0",
            "UPDATE jobs SET agent_id = ? WHERE agent_id = 0",
        ] {
            sqlx::query(statement)
                .bind(repaired_id)
                .execute(&mut *tx)
                .await
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?;
        }

        tx.commit().await.map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;

        Ok(())
    }

    /// Insert a legacy placeholder agent for testing the repair mechanism.
    pub async fn insert_legacy_agent_for_test(&self) -> DbResult<()> {
        sqlx::query(
            r#"
            INSERT INTO agents (id, display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature, created_at, updated_at)
            VALUES (0, 'Legacy Zero Agent', 'legacy', '1.0.0', NULL, 'prompt', '[]', NULL, NULL, datetime('now'), datetime('now'))
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn build_script_watches_migrations_directory() {
        let build_script =
            std::fs::read_to_string(format!("{}/build.rs", env!("CARGO_MANIFEST_DIR")))
                .expect("argus-repository build.rs should exist");

        assert!(
            build_script.contains("cargo:rerun-if-changed=migrations"),
            "build.rs should watch the migrations directory so embedded sqlx migrations stay fresh after rebases"
        );
    }

    #[test]
    fn persistent_workflow_migration_rebuilds_workflows_with_composite_template_fk() {
        let migration =
            std::fs::read_to_string(format!("{}/migrations/20260329160000_persistent_workflow.sql", env!("CARGO_MANIFEST_DIR")))
                .expect("persistent workflow migration should exist");

        assert!(
            migration.contains("CREATE TABLE workflows_new"),
            "workflows should be rebuilt instead of altered in place"
        );
        assert!(
            migration.contains("CHECK (")
                && migration.contains("template_id IS NULL AND template_version IS NULL")
                && migration.contains("template_id IS NOT NULL AND template_version IS NOT NULL"),
            "workflows should require template_id/template_version to be both null or both set"
        );
        assert!(
            migration.contains("FOREIGN KEY (template_id, template_version) REFERENCES workflow_templates(id, version)"),
            "workflows should reference the exact immutable template version"
        );
        assert!(
            migration.contains("INSERT INTO workflows_new"),
            "existing workflow rows should be copied into the rebuilt table"
        );
        assert!(
            migration.contains("ALTER TABLE workflows_new RENAME TO workflows"),
            "rebuilt workflows table should replace the old table"
        );
    }
}
