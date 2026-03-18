//! SQLite implementations of repository traits.

use std::collections::HashMap;
use std::path::Path;
use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use sqlx::{Row, SqlitePool};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePoolOptions};

use crate::error::DbError;
use crate::types::*;
use crate::traits::*;
use argus_protocol::ThreadId;
use argus_protocol::llm::{
    LlmProviderId, LlmProviderKind, LlmProviderKindParseError, LlmProviderRecord, LlmProviderSummary,
    ProviderSecretStatus, SecretString,
};
use argus_llm::secret::{
    ApiKeyCipher, KeyMaterialSource, FileKeyMaterialSource,
    HostMacAddressKeyMaterialSource, StaticKeyMaterialSource,
};

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
    write_cipher: ApiKeyCipher,
    read_ciphers: Vec<ApiKeyCipher>,
}

impl ArgusSqlite {
    /// Create a new ArgusSqlite with default key sources.
    #[must_use]
    pub fn new(pool: SqlitePool) -> Self {
        Self::with_key_sources(
            pool,
            Arc::new(FileKeyMaterialSource::from_env_or_default()),
            vec![Arc::new(HostMacAddressKeyMaterialSource)],
        )
    }

    /// Create an ArgusSqlite with a specific key material.
    #[must_use]
    pub fn new_with_key_material(pool: SqlitePool, key_material: Vec<u8>) -> Self {
        Self::with_key_sources(
            pool,
            Arc::new(StaticKeyMaterialSource::new(key_material)),
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
            .map(|km| Arc::new(StaticKeyMaterialSource::new(km)) as Arc<dyn KeyMaterialSource>)
            .collect();

        Self::with_key_sources(
            pool,
            Arc::new(StaticKeyMaterialSource::new(key_material)),
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
        let mut read_ciphers = vec![ApiKeyCipher::new_arc(Arc::clone(&key_source))];
        read_ciphers.extend(fallback_sources.into_iter().map(ApiKeyCipher::new_arc));

        Self {
            pool,
            write_cipher: ApiKeyCipher::new_arc(key_source),
            read_ciphers,
        }
    }

    /// Get a reference to the underlying pool.
    #[must_use]
    pub fn pool(&self) -> &SqlitePool {
        &self.pool
    }

    // === Helper methods ===

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

// === LlmProviderRepository ===

#[async_trait]
impl LlmProviderRepository for ArgusSqlite {
    async fn upsert_provider(&self, record: &LlmProviderRecord) -> DbResult<LlmProviderId> {
        if record.models.is_empty() {
            return Err(DbError::QueryFailed {
                reason: "At least one model is required".to_string(),
            });
        }
        if !record.models.contains(&record.default_model) {
            return Err(DbError::QueryFailed {
                reason: format!("Default model '{}' must be in models list", record.default_model),
            });
        }

        let encrypted = self.write_cipher.encrypt(record.api_key.expose_secret())
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        let extra_headers_json = serde_json::to_string(&record.extra_headers)
            .map_err(|e| DbError::QueryFailed { reason: format!("failed to serialize extra_headers: {e}") })?;
        let models_json = serde_json::to_string(&record.models)
            .map_err(|e| DbError::QueryFailed { reason: format!("failed to serialize models: {e}") })?;

        let mut tx = self.pool.begin().await.map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        if record.is_default {
            sqlx::query("UPDATE llm_providers SET is_default = 0, updated_at = CURRENT_TIMESTAMP WHERE id != ?1 AND is_default = 1")
                .bind(record.id.into_inner())
                .execute(&mut *tx)
                .await
                .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        }

        let provider_id = if record.id.into_inner() == 0 {
            sqlx::query(
                "INSERT INTO llm_providers (kind, display_name, base_url, models, default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            )
            .bind(record.kind.as_str())
            .bind(&record.display_name)
            .bind(&record.base_url)
            .bind(&models_json)
            .bind(&record.default_model)
            .bind(&encrypted.ciphertext)
            .bind(&encrypted.nonce)
            .bind(i64::from(record.is_default))
            .bind(&extra_headers_json)
            .execute(&mut *tx)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

            let new_id: i64 = sqlx::query_scalar("SELECT last_insert_rowid()")
                .fetch_one(&mut *tx)
                .await
                .map_err(|e| DbError::QueryFailed { reason: format!("failed to get last_insert_rowid: {e}") })?;

            LlmProviderId::new(new_id)
        } else {
            sqlx::query(
                "INSERT INTO llm_providers (id, kind, display_name, base_url, models, default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)
                 ON CONFLICT(id) DO UPDATE SET
                     kind = excluded.kind,
                     display_name = excluded.display_name,
                     base_url = excluded.base_url,
                     models = excluded.models,
                     default_model = excluded.default_model,
                     encrypted_api_key = excluded.encrypted_api_key,
                     api_key_nonce = excluded.api_key_nonce,
                     is_default = excluded.is_default,
                     extra_headers = excluded.extra_headers,
                     updated_at = CURRENT_TIMESTAMP",
            )
            .bind(record.id.into_inner())
            .bind(record.kind.as_str())
            .bind(&record.display_name)
            .bind(&record.base_url)
            .bind(&models_json)
            .bind(&record.default_model)
            .bind(&encrypted.ciphertext)
            .bind(&encrypted.nonce)
            .bind(i64::from(record.is_default))
            .bind(&extra_headers_json)
            .execute(&mut *tx)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

            record.id
        };

        tx.commit().await.map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        Ok(provider_id)
    }

    async fn delete_provider(&self, id: &LlmProviderId) -> DbResult<bool> {
        let result = sqlx::query("DELETE FROM llm_providers WHERE id = ?1")
            .bind(id.into_inner())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(result.rows_affected() > 0)
    }

    async fn set_default_provider(&self, id: &LlmProviderId) -> DbResult<()> {
        let mut tx = self.pool.begin().await.map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        let exists: i64 = sqlx::query_scalar("SELECT count(1) FROM llm_providers WHERE id = ?1")
            .bind(id.into_inner())
            .fetch_one(&mut *tx)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        if exists == 0 {
            return Err(DbError::NotFound { id: id.into_inner().to_string() });
        }

        sqlx::query("UPDATE llm_providers SET is_default = 0, updated_at = CURRENT_TIMESTAMP WHERE is_default = 1")
            .execute(&mut *tx)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        sqlx::query("UPDATE llm_providers SET is_default = 1, updated_at = CURRENT_TIMESTAMP WHERE id = ?1")
            .bind(id.into_inner())
            .execute(&mut *tx)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        tx.commit().await.map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        Ok(())
    }

    async fn get_provider(&self, id: &LlmProviderId) -> DbResult<Option<LlmProviderRecord>> {
        let row = sqlx::query(
            "SELECT id, kind, display_name, base_url, models, default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers
             FROM llm_providers WHERE id = ?1",
        )
        .bind(id.into_inner())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        row.map(|r| self.map_llm_record(r)).transpose()
    }

    async fn get_provider_summary(&self, id: &LlmProviderId) -> DbResult<Option<LlmProviderSummary>> {
        let row = sqlx::query(
            "SELECT id, kind, display_name, base_url, models, default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers
             FROM llm_providers WHERE id = ?1",
        )
        .bind(id.into_inner())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        row.map(|r| self.map_llm_summary(r)).transpose()
    }

    async fn list_providers(&self) -> DbResult<Vec<LlmProviderSummary>> {
        let rows = sqlx::query(
            "SELECT id, kind, display_name, base_url, models, default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers
             FROM llm_providers ORDER BY display_name ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        rows.into_iter().map(|r| self.map_llm_summary(r)).collect()
    }

    async fn get_default_provider(&self) -> DbResult<Option<LlmProviderRecord>> {
        let row = sqlx::query(
            "SELECT id, kind, display_name, base_url, models, default_model, encrypted_api_key, api_key_nonce, is_default, extra_headers
             FROM llm_providers WHERE is_default = 1 LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        row.map(|r| self.map_llm_record(r)).transpose()
    }
}

impl ArgusSqlite {
    fn parse_llm_shared_fields(row: sqlx::sqlite::SqliteRow) -> DbResult<(LlmProviderId, LlmProviderKind, String, String, Vec<String>, String, bool, HashMap<String, String>, Vec<u8>, Vec<u8>)> {
        let nonce: Vec<u8> = Self::get_column(&row, "api_key_nonce")?;
        let ciphertext: Vec<u8> = Self::get_column(&row, "encrypted_api_key")?;
        let extra_headers: HashMap<String, String> = serde_json::from_str(&Self::get_column::<String>(&row, "extra_headers")?)
            .map_err(|e| DbError::QueryFailed { reason: format!("failed to parse extra_headers: {e}") })?;
        let models: Vec<String> = serde_json::from_str(&Self::get_column::<String>(&row, "models")?)
            .map_err(|e| DbError::QueryFailed { reason: format!("failed to parse models: {e}") })?;
        let kind: LlmProviderKind = Self::get_column::<String>(&row, "kind")?.parse()
            .map_err(|e: LlmProviderKindParseError| DbError::InvalidProviderKind { kind: e.to_string() })?;

        Ok((
            LlmProviderId::new(Self::get_column(&row, "id")?),
            kind,
            Self::get_column(&row, "display_name")?,
            Self::get_column(&row, "base_url")?,
            models,
            Self::get_column(&row, "default_model")?,
            Self::get_column::<i64>(&row, "is_default")? != 0,
            extra_headers,
            nonce,
            ciphertext,
        ))
    }

    fn map_llm_record(&self, row: sqlx::sqlite::SqliteRow) -> DbResult<LlmProviderRecord> {
        let (id, kind, display_name, base_url, models, default_model, is_default, extra_headers, nonce, ciphertext) =
            Self::parse_llm_shared_fields(row)?;

        Ok(LlmProviderRecord {
            id,
            kind,
            display_name,
            base_url,
            api_key: self.decrypt_secret(&nonce, &ciphertext)?,
            models,
            default_model,
            is_default,
            extra_headers,
            secret_status: ProviderSecretStatus::Ready,
        })
    }

    fn map_llm_summary(&self, row: sqlx::sqlite::SqliteRow) -> DbResult<LlmProviderSummary> {
        let (id, kind, display_name, base_url, models, default_model, is_default, extra_headers, nonce, ciphertext) =
            Self::parse_llm_shared_fields(row)?;

        let secret_status = if self.decrypt_secret(&nonce, &ciphertext).is_ok() {
            ProviderSecretStatus::Ready
        } else {
            ProviderSecretStatus::RequiresReentry
        };

        Ok(LlmProviderSummary {
            id,
            kind,
            display_name,
            base_url,
            models,
            default_model,
            is_default,
            extra_headers,
            secret_status,
        })
    }
}

// === AgentRepository ===

#[async_trait]
impl AgentRepository for ArgusSqlite {
    async fn upsert(&self, record: &AgentRecord) -> DbResult<()> {
        let tool_names_json = serde_json::to_string(&record.tool_names)
            .map_err(|e| DbError::QueryFailed { reason: format!("failed to serialize tool_names: {e}") })?;
        let temperature_int = record.temperature.map(|t| (t * 100.0) as i64);

        if record.id.into_inner() == 0 {
            sqlx::query(
                "INSERT INTO agents (display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            )
            .bind(&record.display_name)
            .bind(&record.description)
            .bind(&record.version)
            .bind(record.provider_id.as_ref().map(|id| id.into_inner()))
            .bind(&record.system_prompt)
            .bind(&tool_names_json)
            .bind(record.max_tokens.map(|t| t as i64))
            .bind(temperature_int)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        } else {
            sqlx::query(
                "INSERT INTO agents (id, display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(id) DO UPDATE SET
                     display_name = excluded.display_name,
                     description = excluded.description,
                     version = excluded.version,
                     provider_id = excluded.provider_id,
                     system_prompt = excluded.system_prompt,
                     tool_names = excluded.tool_names,
                     max_tokens = excluded.max_tokens,
                     temperature = excluded.temperature,
                     updated_at = CURRENT_TIMESTAMP",
            )
            .bind(record.id.into_inner())
            .bind(&record.display_name)
            .bind(&record.description)
            .bind(&record.version)
            .bind(record.provider_id.as_ref().map(|id| id.into_inner()))
            .bind(&record.system_prompt)
            .bind(&tool_names_json)
            .bind(record.max_tokens.map(|t| t as i64))
            .bind(temperature_int)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        }

        Ok(())
    }

    async fn get(&self, id: &AgentId) -> DbResult<Option<AgentRecord>> {
        let row = sqlx::query(
            "SELECT id, display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature
             FROM agents WHERE id = ?1",
        )
        .bind(id.into_inner())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        row.map(|r| self.map_agent_record(r)).transpose()
    }

    async fn find_by_display_name(&self, display_name: &str) -> DbResult<Option<AgentRecord>> {
        let row = sqlx::query(
            "SELECT id, display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature
             FROM agents WHERE display_name = ?1 LIMIT 1",
        )
        .bind(display_name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        row.map(|r| self.map_agent_record(r)).transpose()
    }

    async fn list(&self) -> DbResult<Vec<AgentRecord>> {
        let rows = sqlx::query(
            "SELECT id, display_name, description, version, provider_id, system_prompt, tool_names, max_tokens, temperature
             FROM agents ORDER BY display_name ASC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        rows.into_iter().map(|r| self.map_agent_record(r)).collect()
    }

    async fn delete(&self, id: &AgentId) -> DbResult<bool> {
        let result = sqlx::query("DELETE FROM agents WHERE id = ?1")
            .bind(id.into_inner())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(result.rows_affected() > 0)
    }
}

impl ArgusSqlite {
    fn map_agent_record(&self, row: sqlx::sqlite::SqliteRow) -> DbResult<AgentRecord> {
        let tool_names: Vec<String> = serde_json::from_str(&Self::get_column::<String>(&row, "tool_names")?)
            .map_err(|e| DbError::QueryFailed { reason: format!("failed to parse tool_names: {e}") })?;
        let temperature: Option<f32> = Self::get_column::<Option<i64>>(&row, "temperature")?
            .map(|t| t as f32 / 100.0);
        let provider_id: Option<i64> = Self::get_column::<Option<i64>>(&row, "provider_id")?;

        Ok(AgentRecord {
            id: AgentId::new(Self::get_column(&row, "id")?),
            display_name: Self::get_column(&row, "display_name")?,
            description: Self::get_column(&row, "description")?,
            version: Self::get_column(&row, "version")?,
            provider_id: provider_id.map(LlmProviderId::new),
            system_prompt: Self::get_column(&row, "system_prompt")?,
            tool_names,
            max_tokens: Self::get_column::<Option<i64>>(&row, "max_tokens")?.map(|t| t as u32),
            temperature,
        })
    }
}

// === ThreadRepository ===

#[async_trait]
impl ThreadRepository for ArgusSqlite {
    async fn upsert_thread(&self, thread: &ThreadRecord) -> DbResult<()> {
        sqlx::query(
            "INSERT INTO threads (id, provider_id, title, token_count, turn_count, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET
                 provider_id = excluded.provider_id,
                 title = excluded.title,
                 token_count = excluded.token_count,
                 turn_count = excluded.turn_count,
                 updated_at = excluded.updated_at",
        )
        .bind(thread.id.to_string())
        .bind(thread.provider_id.into_inner())
        .bind(&thread.title)
        .bind(thread.token_count as i64)
        .bind(thread.turn_count as i64)
        .bind(&thread.created_at)
        .bind(&thread.updated_at)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(())
    }

    async fn get_thread(&self, id: &ThreadId) -> DbResult<Option<ThreadRecord>> {
        let row = sqlx::query(
            "SELECT id, provider_id, title, token_count, turn_count, created_at, updated_at
             FROM threads WHERE id = ?1",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        row.map(|r| self.map_thread_record(r)).transpose()
    }

    async fn list_threads(&self, limit: u32) -> DbResult<Vec<ThreadRecord>> {
        let rows = sqlx::query(
            "SELECT id, provider_id, title, token_count, turn_count, created_at, updated_at
             FROM threads ORDER BY updated_at DESC LIMIT ?1",
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        rows.into_iter().map(|r| self.map_thread_record(r)).collect()
    }

    async fn delete_thread(&self, id: &ThreadId) -> DbResult<bool> {
        let result = sqlx::query("DELETE FROM threads WHERE id = ?1")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(result.rows_affected() > 0)
    }

    async fn add_message(&self, message: &MessageRecord) -> DbResult<MessageId> {
        let result = sqlx::query(
            "INSERT INTO messages (thread_id, seq, role, content, tool_call_id, tool_name, tool_calls, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        )
        .bind(message.thread_id.to_string())
        .bind(message.seq as i64)
        .bind(&message.role)
        .bind(&message.content)
        .bind(&message.tool_call_id)
        .bind(&message.tool_name)
        .bind(&message.tool_calls)
        .bind(&message.created_at)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(MessageId::new(result.last_insert_rowid()))
    }

    async fn get_messages(&self, thread_id: &ThreadId) -> DbResult<Vec<MessageRecord>> {
        let rows = sqlx::query(
            "SELECT id, thread_id, seq, role, content, tool_call_id, tool_name, tool_calls, created_at
             FROM messages WHERE thread_id = ?1 ORDER BY seq ASC",
        )
        .bind(thread_id.to_string())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        rows.into_iter().map(|r| self.map_message_record(r)).collect()
    }

    async fn get_recent_messages(&self, thread_id: &ThreadId, limit: u32) -> DbResult<Vec<MessageRecord>> {
        let rows = sqlx::query(
            "SELECT id, thread_id, seq, role, content, tool_call_id, tool_name, tool_calls, created_at
             FROM messages WHERE thread_id = ?1 ORDER BY seq DESC LIMIT ?2",
        )
        .bind(thread_id.to_string())
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        let mut messages: Vec<MessageRecord> = rows.into_iter()
            .map(|r| self.map_message_record(r))
            .collect::<DbResult<Vec<_>>>()?;
        messages.reverse();
        Ok(messages)
    }

    async fn delete_messages_before(&self, thread_id: &ThreadId, seq: u32) -> DbResult<u64> {
        let result = sqlx::query("DELETE FROM messages WHERE thread_id = ?1 AND seq < ?2")
            .bind(thread_id.to_string())
            .bind(seq as i64)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(result.rows_affected())
    }

    async fn update_thread_stats(&self, id: &ThreadId, token_count: u32, turn_count: u32) -> DbResult<()> {
        sqlx::query(
            "UPDATE threads SET token_count = ?1, turn_count = ?2, updated_at = CURRENT_TIMESTAMP WHERE id = ?3",
        )
        .bind(token_count as i64)
        .bind(turn_count as i64)
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(())
    }
}

impl ArgusSqlite {
    fn map_thread_record(&self, row: sqlx::sqlite::SqliteRow) -> DbResult<ThreadRecord> {
        Ok(ThreadRecord {
            id: ThreadId::parse(&Self::get_column::<String>(&row, "id")?)
                .map_err(|e| DbError::QueryFailed { reason: format!("invalid thread id: {e}") })?,
            provider_id: LlmProviderId::new(Self::get_column(&row, "provider_id")?),
            title: Self::get_column(&row, "title")?,
            token_count: Self::get_column::<i64>(&row, "token_count")? as u32,
            turn_count: Self::get_column::<i64>(&row, "turn_count")? as u32,
            created_at: Self::get_column(&row, "created_at")?,
            updated_at: Self::get_column(&row, "updated_at")?,
        })
    }

    fn map_message_record(&self, row: sqlx::sqlite::SqliteRow) -> DbResult<MessageRecord> {
        Ok(MessageRecord {
            id: Some(MessageId::new(Self::get_column(&row, "id")?)),
            thread_id: ThreadId::parse(&Self::get_column::<String>(&row, "thread_id")?)
                .map_err(|e| DbError::QueryFailed { reason: format!("invalid thread id: {e}") })?,
            seq: Self::get_column::<i64>(&row, "seq")? as u32,
            role: Self::get_column(&row, "role")?,
            content: Self::get_column(&row, "content")?,
            tool_call_id: Self::get_column(&row, "tool_call_id")?,
            tool_name: Self::get_column(&row, "tool_name")?,
            tool_calls: Self::get_column(&row, "tool_calls")?,
            created_at: Self::get_column(&row, "created_at")?,
        })
    }
}

// === JobRepository ===

#[async_trait]
impl JobRepository for ArgusSqlite {
    async fn create(&self, job: &JobRecord) -> DbResult<()> {
        let depends_on_json = serde_json::to_string(
            &job.depends_on.iter().map(|id| id.to_string()).collect::<Vec<_>>()
        ).unwrap_or_else(|_| "[]".to_string());

        sqlx::query(
            "INSERT INTO jobs (id, job_type, name, status, agent_id, context, prompt, thread_id, group_id, depends_on, cron_expr, scheduled_at, started_at, finished_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        )
        .bind(job.id.to_string())
        .bind(job.job_type.as_str())
        .bind(&job.name)
        .bind(job.status.as_str())
        .bind(job.agent_id.into_inner())
        .bind(&job.context)
        .bind(&job.prompt)
        .bind(job.thread_id.map(|t| t.to_string()))
        .bind(&job.group_id)
        .bind(&depends_on_json)
        .bind(&job.cron_expr)
        .bind(&job.scheduled_at)
        .bind(&job.started_at)
        .bind(&job.finished_at)
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(())
    }

    async fn get(&self, id: &JobId) -> DbResult<Option<JobRecord>> {
        let row = sqlx::query(
            "SELECT id, job_type, name, status, agent_id, context, prompt, thread_id, group_id, depends_on, cron_expr, scheduled_at, started_at, finished_at
             FROM jobs WHERE id = ?1",
        )
        .bind(id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        row.map(|r| self.map_job_record(r)).transpose()
    }

    async fn update_status(&self, id: &JobId, status: WorkflowStatus, started_at: Option<&str>, finished_at: Option<&str>) -> DbResult<()> {
        let result = sqlx::query(
            "UPDATE jobs SET status = ?1, started_at = ?2, finished_at = ?3, updated_at = datetime('now')
             WHERE id = ?4 AND status NOT IN ('succeeded', 'failed', 'cancelled')",
        )
        .bind(status.as_str())
        .bind(started_at)
        .bind(finished_at)
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        if result.rows_affected() == 0 {
            return Err(DbError::QueryFailed { reason: format!("job {} not found or in terminal state", id) });
        }

        Ok(())
    }

    async fn update_thread_id(&self, id: &JobId, thread_id: &ThreadId) -> DbResult<()> {
        sqlx::query("UPDATE jobs SET thread_id = ?1, updated_at = datetime('now') WHERE id = ?2")
            .bind(thread_id.to_string())
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(())
    }

    async fn find_ready_jobs(&self, limit: usize) -> DbResult<Vec<JobRecord>> {
        let rows = sqlx::query(
            "SELECT j.id, j.job_type, j.name, j.status, j.agent_id, j.context, j.prompt, j.thread_id, j.group_id, j.depends_on, j.cron_expr, j.scheduled_at, j.started_at, j.finished_at
             FROM jobs j
             WHERE j.status = 'pending' AND j.job_type != 'cron'
               AND NOT EXISTS (
                   SELECT 1 FROM jobs dep
                   WHERE dep.id IN (SELECT value FROM json_each(j.depends_on))
                     AND dep.status != 'succeeded'
               )
             ORDER BY j.created_at ASC LIMIT ?1",
        )
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        rows.into_iter().map(|r| self.map_job_record(r)).collect()
    }

    async fn find_due_cron_jobs(&self, now: &str) -> DbResult<Vec<JobRecord>> {
        let rows = sqlx::query(
            "SELECT id, job_type, name, status, agent_id, context, prompt, thread_id, group_id, depends_on, cron_expr, scheduled_at, started_at, finished_at
             FROM jobs
             WHERE job_type = 'cron' AND scheduled_at IS NOT NULL AND scheduled_at <= ?1
             ORDER BY scheduled_at ASC",
        )
        .bind(now)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        rows.into_iter().map(|r| self.map_job_record(r)).collect()
    }

    async fn update_scheduled_at(&self, id: &JobId, next: &str) -> DbResult<()> {
        sqlx::query("UPDATE jobs SET scheduled_at = ?1, updated_at = datetime('now') WHERE id = ?2")
            .bind(next)
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(())
    }

    async fn list_by_group(&self, group_id: &str) -> DbResult<Vec<JobRecord>> {
        let rows = sqlx::query(
            "SELECT id, job_type, name, status, agent_id, context, prompt, thread_id, group_id, depends_on, cron_expr, scheduled_at, started_at, finished_at
             FROM jobs WHERE group_id = ?1 ORDER BY created_at ASC",
        )
        .bind(group_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        rows.into_iter().map(|r| self.map_job_record(r)).collect()
    }

    async fn delete(&self, id: &JobId) -> DbResult<bool> {
        let result = sqlx::query("DELETE FROM jobs WHERE id = ?1")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(result.rows_affected() > 0)
    }
}

impl ArgusSqlite {
    fn map_job_record(&self, row: sqlx::sqlite::SqliteRow) -> DbResult<JobRecord> {
        let depends_on: Vec<JobId> = serde_json::from_str::<Vec<String>>(&Self::get_column::<String>(&row, "depends_on")?)
            .map(|ids| ids.into_iter().map(JobId::new).collect())
            .unwrap_or_default();
        let thread_id: Option<ThreadId> = Self::get_column::<Option<String>>(&row, "thread_id")?
            .and_then(|s| ThreadId::parse(&s).ok());

        Ok(JobRecord {
            id: JobId::new(&Self::get_column::<String>(&row, "id")?),
            job_type: JobType::parse_str(&Self::get_column::<String>(&row, "job_type")?)
                .map_err(|e| DbError::QueryFailed { reason: e })?,
            name: Self::get_column(&row, "name")?,
            status: WorkflowStatus::parse_str(&Self::get_column::<String>(&row, "status")?)
                .map_err(|e| DbError::QueryFailed { reason: e })?,
            agent_id: AgentId::new(Self::get_column(&row, "agent_id")?),
            context: Self::get_column(&row, "context")?,
            prompt: Self::get_column(&row, "prompt")?,
            thread_id,
            group_id: Self::get_column(&row, "group_id")?,
            depends_on,
            cron_expr: Self::get_column(&row, "cron_expr")?,
            scheduled_at: Self::get_column(&row, "scheduled_at")?,
            started_at: Self::get_column(&row, "started_at")?,
            finished_at: Self::get_column(&row, "finished_at")?,
        })
    }
}

// === WorkflowRepository ===

#[async_trait]
impl WorkflowRepository for ArgusSqlite {
    async fn create_workflow(&self, workflow: &WorkflowRecord) -> DbResult<()> {
        sqlx::query("INSERT INTO workflows (id, name, status) VALUES (?1, ?2, ?3)")
            .bind(workflow.id.as_ref())
            .bind(&workflow.name)
            .bind(workflow.status.as_str())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(())
    }

    async fn get_workflow(&self, id: &WorkflowId) -> DbResult<Option<WorkflowRecord>> {
        let row = sqlx::query("SELECT id, name, status FROM workflows WHERE id = ?1")
            .bind(id.as_ref())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        row.map(|r| self.map_workflow_record(r)).transpose()
    }

    async fn update_workflow_status(&self, id: &WorkflowId, status: WorkflowStatus) -> DbResult<()> {
        let result = sqlx::query("UPDATE workflows SET status = ?1 WHERE id = ?2")
            .bind(status.as_str())
            .bind(id.as_ref())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        if result.rows_affected() == 0 {
            return Err(DbError::QueryFailed { reason: format!("workflow not found: {}", id) });
        }

        Ok(())
    }

    async fn list_workflows(&self) -> DbResult<Vec<WorkflowRecord>> {
        let rows = sqlx::query("SELECT id, name, status FROM workflows ORDER BY name")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        rows.into_iter().map(|r| self.map_workflow_record(r)).collect()
    }

    async fn delete_workflow(&self, id: &WorkflowId) -> DbResult<bool> {
        let result = sqlx::query("DELETE FROM workflows WHERE id = ?1")
            .bind(id.as_ref())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;

        Ok(result.rows_affected() > 0)
    }
}

impl ArgusSqlite {
    fn map_workflow_record(&self, row: sqlx::sqlite::SqliteRow) -> DbResult<WorkflowRecord> {
        Ok(WorkflowRecord {
            id: WorkflowId::new(Self::get_column::<String>(&row, "id")?),
            name: Self::get_column(&row, "name")?,
            status: WorkflowStatus::parse_str(&Self::get_column::<String>(&row, "status")?)
                .map_err(|e| DbError::QueryFailed { reason: e })?,
        })
    }
}
