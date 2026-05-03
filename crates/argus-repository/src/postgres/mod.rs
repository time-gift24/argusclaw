//! PostgreSQL implementations of repository traits for the server/web runtime.

use std::str::FromStr;
use std::sync::Arc;

use async_trait::async_trait;
use sqlx::Row;
use sqlx::postgres::{PgPool, PgPoolOptions, PgRow};
use uuid::Uuid;

use argus_crypto::{Cipher, FileKeySource, KeyMaterialSource, StaticKeySource};
use argus_protocol::account::{AccountCredentials, AccountRepository};
use argus_protocol::llm::{
    LlmProviderId, LlmProviderKind, LlmProviderRecord, LlmProviderRepository, ModelConfig,
    ProviderSecretStatus, SecretString,
};
use argus_protocol::{
    AgentId, AgentMcpBinding, AgentMcpServerBinding, AgentRecord, ArgusError,
    McpDiscoveredToolRecord, McpServerRecord, McpServerStatus, ProviderId, SessionId, ThreadId,
    UserId,
};

use crate::error::DbError;
use crate::traits::{
    AgentRepository, AgentRunRepository, JobRepository, McpRepository, SessionRepository,
    SessionWithCount, TemplateRepairRepository, ThreadRepository, UserRepository,
};
use crate::types::{
    AgentRunId, AgentRunRecord, AgentRunStatus, JobId, JobRecord, JobResult, JobStatus, JobType,
    MessageId, MessageRecord, SessionRecord, ThreadRecord,
};

type DbResult<T> = std::result::Result<T, DbError>;
const LEGACY_USER_EXTERNAL_ID: &str = "__legacy__";

pub async fn connect(database_url: &str) -> DbResult<PgPool> {
    PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await
        .map_err(|e| DbError::ConnectionFailed {
            reason: e.to_string(),
        })
}

pub async fn migrate(pool: &PgPool) -> DbResult<()> {
    sqlx::migrate!("./postgres_migrations")
        .run(pool)
        .await
        .map_err(|e| DbError::MigrationFailed {
            reason: e.to_string(),
        })
}

pub struct ArgusPostgres {
    pool: PgPool,
    write_cipher: Cipher,
    read_ciphers: Vec<Cipher>,
}

impl ArgusPostgres {
    #[must_use]
    pub fn new(pool: PgPool) -> Self {
        Self::with_key_sources(
            pool,
            Arc::new(FileKeySource::from_env_or_default()),
            vec![Arc::new(FileKeySource::from_env_or_default())],
        )
    }

    #[must_use]
    pub fn new_with_key_material(pool: PgPool, key_material: Vec<u8>) -> Self {
        Self::with_key_sources(
            pool,
            Arc::new(StaticKeySource::new(key_material)),
            Vec::new(),
        )
    }

    #[must_use]
    pub fn with_key_sources(
        pool: PgPool,
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

    #[must_use]
    pub fn pool(&self) -> &PgPool {
        &self.pool
    }

    fn get<T>(row: &PgRow, col: &str) -> DbResult<T>
    where
        T: for<'r> sqlx::decode::Decode<'r, sqlx::Postgres> + sqlx::types::Type<sqlx::Postgres>,
    {
        row.try_get(col).map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })
    }

    async fn resolve_legacy_user(&self) -> DbResult<UserId> {
        let row = sqlx::query(
            "INSERT INTO users (id, external_id, created_at, updated_at)
             VALUES ($1, $2, CURRENT_TIMESTAMP::TEXT, CURRENT_TIMESTAMP::TEXT)
             ON CONFLICT(external_id) DO UPDATE SET updated_at = users.updated_at
             RETURNING id",
        )
        .bind(Uuid::nil())
        .bind(LEGACY_USER_EXTERNAL_ID)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        Ok(UserId(Self::get(&row, "id")?))
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

    fn parse_session_id(value: Uuid) -> SessionId {
        SessionId(value)
    }
    fn parse_thread_id(value: Uuid) -> ThreadId {
        ThreadId(value)
    }
    fn parse_user_id(value: Uuid) -> UserId {
        UserId(value)
    }

    fn map_session_record(&self, row: &PgRow) -> DbResult<SessionRecord> {
        Ok(SessionRecord {
            id: Self::parse_session_id(Self::get(row, "id")?),
            name: Self::get(row, "name")?,
            created_at: Self::get(row, "created_at")?,
            updated_at: Self::get(row, "updated_at")?,
        })
    }

    fn map_thread_record(&self, row: PgRow) -> DbResult<ThreadRecord> {
        let session_id: Option<Uuid> = Self::get(&row, "session_id")?;
        let template_id: Option<i64> = Self::get(&row, "template_id")?;
        Ok(ThreadRecord {
            id: Self::parse_thread_id(Self::get(&row, "id")?),
            provider_id: LlmProviderId::new(Self::get(&row, "provider_id")?),
            title: Self::get(&row, "title")?,
            token_count: Self::get::<i64>(&row, "token_count")? as u32,
            turn_count: Self::get::<i64>(&row, "turn_count")? as u32,
            session_id: session_id.map(Self::parse_session_id),
            template_id: template_id.map(AgentId::new),
            model_override: Self::get(&row, "model_override")?,
            created_at: Self::get(&row, "created_at")?,
            updated_at: Self::get(&row, "updated_at")?,
        })
    }

    fn map_message_record(&self, row: PgRow) -> DbResult<MessageRecord> {
        Ok(MessageRecord {
            id: Some(MessageId::new(Self::get(&row, "id")?)),
            thread_id: Self::parse_thread_id(Self::get(&row, "thread_id")?),
            seq: Self::get::<i64>(&row, "seq")? as u32,
            role: Self::get(&row, "role")?,
            content: Self::get(&row, "content")?,
            tool_call_id: Self::get(&row, "tool_call_id")?,
            tool_name: Self::get(&row, "tool_name")?,
            tool_calls: Self::get(&row, "tool_calls")?,
            created_at: Self::get(&row, "created_at")?,
        })
    }

    fn serialize_json<T: serde::Serialize>(value: &T) -> DbResult<String> {
        serde_json::to_string(value).map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })
    }

    fn map_agent_record(&self, row: PgRow) -> DbResult<AgentRecord> {
        let tool_names: Vec<String> =
            serde_json::from_str(&Self::get::<String>(&row, "tool_names")?).map_err(|e| {
                DbError::QueryFailed {
                    reason: format!("failed to parse tool_names: {e}"),
                }
            })?;
        let subagent_names: Vec<String> =
            serde_json::from_str(&Self::get::<String>(&row, "subagent_names")?).map_err(|e| {
                DbError::QueryFailed {
                    reason: format!("failed to parse subagent_names: {e}"),
                }
            })?;
        let thinking_config = Self::get::<Option<String>>(&row, "thinking_config")?
            .as_deref()
            .map(serde_json::from_str)
            .transpose()
            .map_err(|e| DbError::QueryFailed {
                reason: format!("failed to parse thinking_config: {e}"),
            })?;
        Ok(AgentRecord {
            id: AgentId::new(Self::get(&row, "id")?),
            display_name: Self::get(&row, "display_name")?,
            description: Self::get(&row, "description")?,
            version: Self::get(&row, "version")?,
            provider_id: Self::get::<Option<i64>>(&row, "provider_id")?.map(ProviderId::new),
            model_id: Self::get(&row, "model_id")?,
            system_prompt: Self::get(&row, "system_prompt")?,
            tool_names,
            subagent_names,
            max_tokens: Self::get::<Option<i64>>(&row, "max_tokens")?.map(|v| v as u32),
            temperature: Self::get::<Option<i64>>(&row, "temperature")?.map(|v| v as f32 / 100.0),
            thinking_config,
        })
    }

    fn map_llm_record(&self, row: PgRow) -> Result<LlmProviderRecord, ArgusError> {
        let nonce: Vec<u8> = Self::get(&row, "api_key_nonce").map_err(ArgusError::from)?;
        let ciphertext: Vec<u8> = Self::get(&row, "encrypted_api_key").map_err(ArgusError::from)?;
        let extra_headers = serde_json::from_str(
            &Self::get::<String>(&row, "extra_headers").map_err(ArgusError::from)?,
        )
        .map_err(|e| {
            ArgusError::from(DbError::QueryFailed {
                reason: format!("failed to parse extra_headers: {e}"),
            })
        })?;
        let meta_data = serde_json::from_str(
            &Self::get::<String>(&row, "meta_data").map_err(ArgusError::from)?,
        )
        .map_err(|e| {
            ArgusError::from(DbError::QueryFailed {
                reason: format!("failed to parse meta_data: {e}"),
            })
        })?;
        let models =
            serde_json::from_str(&Self::get::<String>(&row, "models").map_err(ArgusError::from)?)
                .map_err(|e| {
                ArgusError::from(DbError::QueryFailed {
                    reason: format!("failed to parse models: {e}"),
                })
            })?;
        let model_config = serde_json::from_str::<std::collections::HashMap<String, ModelConfig>>(
            &Self::get::<String>(&row, "model_config").map_err(ArgusError::from)?,
        )
        .map_err(|e| {
            ArgusError::from(DbError::QueryFailed {
                reason: format!("failed to parse model_config: {e}"),
            })
        })?;
        let secret = if ciphertext.is_empty() && nonce.is_empty() {
            SecretString::new("")
        } else {
            self.decrypt_secret(&nonce, &ciphertext)
                .map_err(ArgusError::from)?
        };
        let kind_text: String = Self::get(&row, "kind").map_err(ArgusError::from)?;
        let kind = LlmProviderKind::from_str(&kind_text).map_err(|e| {
            ArgusError::from(DbError::QueryFailed {
                reason: e.to_string(),
            })
        })?;
        Ok(LlmProviderRecord {
            id: LlmProviderId::new(Self::get(&row, "id").map_err(ArgusError::from)?),
            kind,
            display_name: Self::get(&row, "display_name").map_err(ArgusError::from)?,
            base_url: Self::get(&row, "base_url").map_err(ArgusError::from)?,
            api_key: secret,
            models,
            model_config,
            default_model: Self::get(&row, "default_model").map_err(ArgusError::from)?,
            is_default: Self::get(&row, "is_default").map_err(ArgusError::from)?,
            extra_headers,
            secret_status: ProviderSecretStatus::Ready,
            meta_data,
        })
    }

    fn parse_mcp_status(value: &str) -> DbResult<McpServerStatus> {
        match value {
            "ready" => Ok(McpServerStatus::Ready),
            "connecting" => Ok(McpServerStatus::Connecting),
            "retrying" => Ok(McpServerStatus::Retrying),
            "failed" => Ok(McpServerStatus::Failed),
            "disabled" => Ok(McpServerStatus::Disabled),
            _ => Err(DbError::QueryFailed {
                reason: format!("invalid mcp status: {value}"),
            }),
        }
    }

    fn mcp_status_text(status: McpServerStatus) -> &'static str {
        match status {
            McpServerStatus::Ready => "ready",
            McpServerStatus::Connecting => "connecting",
            McpServerStatus::Retrying => "retrying",
            McpServerStatus::Failed => "failed",
            McpServerStatus::Disabled => "disabled",
        }
    }

    fn map_mcp_server(&self, row: PgRow) -> DbResult<McpServerRecord> {
        let transport_json: String = Self::get(&row, "transport")?;
        Ok(McpServerRecord {
            id: Some(Self::get(&row, "id")?),
            display_name: Self::get(&row, "display_name")?,
            enabled: Self::get(&row, "enabled")?,
            transport: serde_json::from_str(&transport_json).map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?,
            timeout_ms: Self::get::<i64>(&row, "timeout_ms")? as u64,
            status: Self::parse_mcp_status(&Self::get::<String>(&row, "status")?)?,
            last_checked_at: Self::get(&row, "last_checked_at")?,
            last_success_at: Self::get(&row, "last_success_at")?,
            last_error: Self::get(&row, "last_error")?,
            discovered_tool_count: Self::get::<i64>(&row, "discovered_tool_count")? as u32,
        })
    }
}

#[async_trait]
impl UserRepository for ArgusPostgres {
    async fn resolve_user(
        &self,
        external_id: &str,
        display_name: Option<&str>,
    ) -> DbResult<UserId> {
        let id = UserId::new();
        let row = sqlx::query(
            "INSERT INTO users (id, external_id, display_name, created_at, updated_at)
             VALUES ($1, $2, $3, CURRENT_TIMESTAMP::TEXT, CURRENT_TIMESTAMP::TEXT)
             ON CONFLICT(external_id) DO UPDATE SET display_name = COALESCE(EXCLUDED.display_name, users.display_name), updated_at = CURRENT_TIMESTAMP::TEXT
             RETURNING id",
        )
        .bind(*id.inner())
        .bind(external_id)
        .bind(display_name)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        Ok(Self::parse_user_id(Self::get(&row, "id")?))
    }
}

#[async_trait]
impl SessionRepository for ArgusPostgres {
    async fn list_with_counts(&self) -> DbResult<Vec<SessionWithCount>> {
        let rows = sqlx::query("SELECT s.id, s.name, s.created_at, s.updated_at, COUNT(t.id)::BIGINT as thread_count FROM sessions s LEFT JOIN threads t ON t.session_id = s.id GROUP BY s.id ORDER BY s.updated_at DESC")
            .fetch_all(&self.pool).await.map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        rows.into_iter()
            .map(|r| {
                Ok(SessionWithCount {
                    session: self.map_session_record(&r)?,
                    thread_count: Self::get(&r, "thread_count")?,
                })
            })
            .collect()
    }
    async fn get(&self, id: &SessionId) -> DbResult<Option<SessionRecord>> {
        let row =
            sqlx::query("SELECT id, name, created_at, updated_at FROM sessions WHERE id = $1")
                .bind(*id.inner())
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?;
        row.map(|r| self.map_session_record(&r)).transpose()
    }
    async fn create(&self, id: &SessionId, name: &str) -> DbResult<()> {
        let legacy_user = self.resolve_legacy_user().await?;
        self.create_for_user(&legacy_user, id, name).await
    }
    async fn rename(&self, id: &SessionId, name: &str) -> DbResult<bool> {
        let result = sqlx::query(
            "UPDATE sessions SET name = $1, updated_at = CURRENT_TIMESTAMP::TEXT WHERE id = $2",
        )
        .bind(name)
        .bind(*id.inner())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        Ok(result.rows_affected() > 0)
    }
    async fn delete(&self, id: &SessionId) -> DbResult<bool> {
        let result = sqlx::query("DELETE FROM sessions WHERE id = $1")
            .bind(*id.inner())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        Ok(result.rows_affected() > 0)
    }
    async fn list_with_counts_for_user(&self, user_id: &UserId) -> DbResult<Vec<SessionWithCount>> {
        let rows = sqlx::query("SELECT s.id, s.name, s.created_at, s.updated_at, COUNT(t.id)::BIGINT as thread_count FROM sessions s LEFT JOIN threads t ON t.session_id = s.id WHERE s.user_id = $1 GROUP BY s.id ORDER BY s.updated_at DESC")
            .bind(*user_id.inner()).fetch_all(&self.pool).await.map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        rows.into_iter()
            .map(|r| {
                Ok(SessionWithCount {
                    session: self.map_session_record(&r)?,
                    thread_count: Self::get(&r, "thread_count")?,
                })
            })
            .collect()
    }
    async fn get_for_user(
        &self,
        user_id: &UserId,
        id: &SessionId,
    ) -> DbResult<Option<SessionRecord>> {
        let row = sqlx::query(
            "SELECT id, name, created_at, updated_at FROM sessions WHERE user_id = $1 AND id = $2",
        )
        .bind(*user_id.inner())
        .bind(*id.inner())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        row.map(|r| self.map_session_record(&r)).transpose()
    }
    async fn create_for_user(&self, user_id: &UserId, id: &SessionId, name: &str) -> DbResult<()> {
        sqlx::query("INSERT INTO sessions (id, user_id, name, created_at, updated_at) VALUES ($1, $2, $3, CURRENT_TIMESTAMP::TEXT, CURRENT_TIMESTAMP::TEXT)")
            .bind(*id.inner()).bind(*user_id.inner()).bind(name).execute(&self.pool).await.map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        Ok(())
    }
    async fn rename_for_user(
        &self,
        user_id: &UserId,
        id: &SessionId,
        name: &str,
    ) -> DbResult<bool> {
        let result = sqlx::query("UPDATE sessions SET name = $1, updated_at = CURRENT_TIMESTAMP::TEXT WHERE user_id = $2 AND id = $3").bind(name).bind(*user_id.inner()).bind(*id.inner()).execute(&self.pool).await.map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        Ok(result.rows_affected() > 0)
    }
    async fn delete_for_user(&self, user_id: &UserId, id: &SessionId) -> DbResult<bool> {
        let result = sqlx::query("DELETE FROM sessions WHERE user_id = $1 AND id = $2")
            .bind(*user_id.inner())
            .bind(*id.inner())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        Ok(result.rows_affected() > 0)
    }
}

#[async_trait]
impl ThreadRepository for ArgusPostgres {
    async fn upsert_thread(&self, thread: &ThreadRecord) -> DbResult<()> {
        sqlx::query("INSERT INTO threads (id, provider_id, title, token_count, turn_count, session_id, template_id, model_override, created_at, updated_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) ON CONFLICT(id) DO UPDATE SET provider_id=EXCLUDED.provider_id,title=EXCLUDED.title,token_count=EXCLUDED.token_count,turn_count=EXCLUDED.turn_count,session_id=EXCLUDED.session_id,template_id=EXCLUDED.template_id,model_override=EXCLUDED.model_override,updated_at=EXCLUDED.updated_at")
            .bind(*thread.id.inner()).bind(thread.provider_id.into_inner()).bind(&thread.title).bind(thread.token_count as i64).bind(thread.turn_count as i64).bind(thread.session_id.map(|id| *id.inner())).bind(thread.template_id.map(|id| id.inner())).bind(&thread.model_override).bind(&thread.created_at).bind(&thread.updated_at).execute(&self.pool).await.map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        Ok(())
    }
    async fn get_thread(&self, id: &ThreadId) -> DbResult<Option<ThreadRecord>> {
        let row = sqlx::query("SELECT id, provider_id, title, token_count, turn_count, session_id, template_id, model_override, created_at, updated_at FROM threads WHERE id=$1").bind(*id.inner()).fetch_optional(&self.pool).await.map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        row.map(|r| self.map_thread_record(r)).transpose()
    }
    async fn list_threads(&self, limit: u32) -> DbResult<Vec<ThreadRecord>> {
        let rows = sqlx::query("SELECT id, provider_id, title, token_count, turn_count, session_id, template_id, model_override, created_at, updated_at FROM threads ORDER BY updated_at DESC LIMIT $1").bind(limit as i64).fetch_all(&self.pool).await.map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        rows.into_iter()
            .map(|r| self.map_thread_record(r))
            .collect()
    }
    async fn list_threads_in_session(&self, session_id: &SessionId) -> DbResult<Vec<ThreadRecord>> {
        self.list_threads_in_session_raw(session_id, None).await
    }
    async fn delete_thread(&self, id: &ThreadId) -> DbResult<bool> {
        let r = sqlx::query("DELETE FROM threads WHERE id=$1")
            .bind(*id.inner())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        Ok(r.rows_affected() > 0)
    }
    async fn delete_threads_in_session(&self, session_id: &SessionId) -> DbResult<u64> {
        let r = sqlx::query("DELETE FROM threads WHERE session_id=$1")
            .bind(*session_id.inner())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        Ok(r.rows_affected())
    }
    async fn add_message(&self, message: &MessageRecord) -> DbResult<MessageId> {
        let row=sqlx::query("INSERT INTO messages (thread_id,seq,role,content,tool_call_id,tool_name,tool_calls,created_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) RETURNING id").bind(*message.thread_id.inner()).bind(message.seq as i64).bind(&message.role).bind(&message.content).bind(&message.tool_call_id).bind(&message.tool_name).bind(&message.tool_calls).bind(&message.created_at).fetch_one(&self.pool).await.map_err(|e| DbError::QueryFailed{reason:e.to_string()})?;
        Ok(MessageId::new(Self::get(&row, "id")?))
    }
    async fn get_messages(&self, thread_id: &ThreadId) -> DbResult<Vec<MessageRecord>> {
        let rows=sqlx::query("SELECT id, thread_id, seq, role, content, tool_call_id, tool_name, tool_calls, created_at FROM messages WHERE thread_id=$1 ORDER BY seq ASC").bind(*thread_id.inner()).fetch_all(&self.pool).await.map_err(|e| DbError::QueryFailed{reason:e.to_string()})?;
        rows.into_iter()
            .map(|r| self.map_message_record(r))
            .collect()
    }
    async fn get_recent_messages(
        &self,
        thread_id: &ThreadId,
        limit: u32,
    ) -> DbResult<Vec<MessageRecord>> {
        let rows=sqlx::query("SELECT id, thread_id, seq, role, content, tool_call_id, tool_name, tool_calls, created_at FROM messages WHERE thread_id=$1 ORDER BY seq DESC LIMIT $2").bind(*thread_id.inner()).bind(limit as i64).fetch_all(&self.pool).await.map_err(|e| DbError::QueryFailed{reason:e.to_string()})?;
        let mut v = rows
            .into_iter()
            .map(|r| self.map_message_record(r))
            .collect::<DbResult<Vec<_>>>()?;
        v.reverse();
        Ok(v)
    }
    async fn delete_messages_before(&self, thread_id: &ThreadId, seq: u32) -> DbResult<u64> {
        let r = sqlx::query("DELETE FROM messages WHERE thread_id=$1 AND seq<$2")
            .bind(*thread_id.inner())
            .bind(seq as i64)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        Ok(r.rows_affected())
    }
    async fn update_thread_stats(
        &self,
        id: &ThreadId,
        token_count: u32,
        turn_count: u32,
    ) -> DbResult<()> {
        sqlx::query("UPDATE threads SET token_count=$1, turn_count=$2, updated_at=CURRENT_TIMESTAMP::TEXT WHERE id=$3").bind(token_count as i64).bind(turn_count as i64).bind(*id.inner()).execute(&self.pool).await.map_err(|e| DbError::QueryFailed{reason:e.to_string()})?;
        Ok(())
    }
    async fn rename_thread(
        &self,
        id: &ThreadId,
        session_id: &SessionId,
        title: Option<&str>,
    ) -> DbResult<bool> {
        let r=sqlx::query("UPDATE threads SET title=$1, updated_at=CURRENT_TIMESTAMP::TEXT WHERE id=$2 AND session_id=$3").bind(title).bind(*id.inner()).bind(*session_id.inner()).execute(&self.pool).await.map_err(|e| DbError::QueryFailed{reason:e.to_string()})?;
        Ok(r.rows_affected() > 0)
    }
    async fn update_thread_model(
        &self,
        id: &ThreadId,
        session_id: &SessionId,
        provider_id: LlmProviderId,
        model_override: Option<&str>,
    ) -> DbResult<bool> {
        let r=sqlx::query("UPDATE threads SET provider_id=$1, model_override=$2, updated_at=CURRENT_TIMESTAMP::TEXT WHERE id=$3 AND session_id=$4").bind(provider_id.into_inner()).bind(model_override).bind(*id.inner()).bind(*session_id.inner()).execute(&self.pool).await.map_err(|e| DbError::QueryFailed{reason:e.to_string()})?;
        Ok(r.rows_affected() > 0)
    }
    async fn get_thread_in_session(
        &self,
        thread_id: &ThreadId,
        session_id: &SessionId,
    ) -> DbResult<Option<ThreadRecord>> {
        let row=sqlx::query("SELECT id, provider_id, title, token_count, turn_count, session_id, template_id, model_override, created_at, updated_at FROM threads WHERE id=$1 AND session_id=$2").bind(*thread_id.inner()).bind(*session_id.inner()).fetch_optional(&self.pool).await.map_err(|e| DbError::QueryFailed{reason:e.to_string()})?;
        row.map(|r| self.map_thread_record(r)).transpose()
    }
    async fn upsert_thread_for_user(
        &self,
        user_id: &UserId,
        thread: &ThreadRecord,
    ) -> DbResult<()> {
        if let Some(session_id) = thread.session_id {
            let owns_session: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM sessions WHERE user_id=$1 AND id=$2)",
            )
            .bind(*user_id.inner())
            .bind(*session_id.inner())
            .fetch_one(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
            if !owns_session {
                return Err(DbError::NotFound {
                    id: session_id.to_string(),
                });
            }
        }
        self.upsert_thread(thread).await
    }
    async fn get_thread_for_user(
        &self,
        user_id: &UserId,
        id: &ThreadId,
    ) -> DbResult<Option<ThreadRecord>> {
        let row = sqlx::query("SELECT t.id, t.provider_id, t.title, t.token_count, t.turn_count, t.session_id, t.template_id, t.model_override, t.created_at, t.updated_at FROM threads t JOIN sessions s ON s.id=t.session_id WHERE s.user_id=$1 AND t.id=$2")
            .bind(*user_id.inner())
            .bind(*id.inner())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        row.map(|r| self.map_thread_record(r)).transpose()
    }
    async fn list_threads_in_session_for_user(
        &self,
        user_id: &UserId,
        session_id: &SessionId,
    ) -> DbResult<Vec<ThreadRecord>> {
        self.list_threads_in_session_raw(session_id, Some(user_id))
            .await
    }
    async fn delete_thread_for_user(&self, user_id: &UserId, id: &ThreadId) -> DbResult<bool> {
        let r=sqlx::query("DELETE FROM threads t USING sessions s WHERE t.session_id=s.id AND s.user_id=$1 AND t.id=$2").bind(*user_id.inner()).bind(*id.inner()).execute(&self.pool).await.map_err(|e| DbError::QueryFailed{reason:e.to_string()})?;
        Ok(r.rows_affected() > 0)
    }
    async fn delete_threads_in_session_for_user(
        &self,
        user_id: &UserId,
        session_id: &SessionId,
    ) -> DbResult<u64> {
        let r=sqlx::query("DELETE FROM threads t USING sessions s WHERE t.session_id=s.id AND s.user_id=$1 AND t.session_id=$2").bind(*user_id.inner()).bind(*session_id.inner()).execute(&self.pool).await.map_err(|e| DbError::QueryFailed{reason:e.to_string()})?;
        Ok(r.rows_affected())
    }
    async fn rename_thread_for_user(
        &self,
        user_id: &UserId,
        id: &ThreadId,
        session_id: &SessionId,
        title: Option<&str>,
    ) -> DbResult<bool> {
        let r=sqlx::query("UPDATE threads t SET title=$1, updated_at=CURRENT_TIMESTAMP::TEXT FROM sessions s WHERE t.session_id=s.id AND s.user_id=$2 AND t.id=$3 AND t.session_id=$4").bind(title).bind(*user_id.inner()).bind(*id.inner()).bind(*session_id.inner()).execute(&self.pool).await.map_err(|e| DbError::QueryFailed{reason:e.to_string()})?;
        Ok(r.rows_affected() > 0)
    }
    async fn update_thread_model_for_user(
        &self,
        user_id: &UserId,
        id: &ThreadId,
        session_id: &SessionId,
        provider_id: LlmProviderId,
        model_override: Option<&str>,
    ) -> DbResult<bool> {
        let r=sqlx::query("UPDATE threads t SET provider_id=$1, model_override=$2, updated_at=CURRENT_TIMESTAMP::TEXT FROM sessions s WHERE t.session_id=s.id AND s.user_id=$3 AND t.id=$4 AND t.session_id=$5").bind(provider_id.into_inner()).bind(model_override).bind(*user_id.inner()).bind(*id.inner()).bind(*session_id.inner()).execute(&self.pool).await.map_err(|e| DbError::QueryFailed{reason:e.to_string()})?;
        Ok(r.rows_affected() > 0)
    }
    async fn get_thread_in_session_for_user(
        &self,
        user_id: &UserId,
        thread_id: &ThreadId,
        session_id: &SessionId,
    ) -> DbResult<Option<ThreadRecord>> {
        let row=sqlx::query("SELECT t.id, t.provider_id, t.title, t.token_count, t.turn_count, t.session_id, t.template_id, t.model_override, t.created_at, t.updated_at FROM threads t JOIN sessions s ON s.id=t.session_id WHERE s.user_id=$1 AND t.id=$2 AND t.session_id=$3").bind(*user_id.inner()).bind(*thread_id.inner()).bind(*session_id.inner()).fetch_optional(&self.pool).await.map_err(|e| DbError::QueryFailed{reason:e.to_string()})?;
        row.map(|r| self.map_thread_record(r)).transpose()
    }
}

impl ArgusPostgres {
    async fn list_threads_in_session_raw(
        &self,
        session_id: &SessionId,
        user_id: Option<&UserId>,
    ) -> DbResult<Vec<ThreadRecord>> {
        let rows = if let Some(user_id) = user_id {
            sqlx::query("SELECT t.id, t.provider_id, t.title, t.token_count, t.turn_count, t.session_id, t.template_id, t.model_override, t.created_at, t.updated_at FROM threads t JOIN sessions s ON s.id=t.session_id WHERE t.session_id=$1 AND s.user_id=$2 ORDER BY t.created_at ASC")
                .bind(*session_id.inner()).bind(*user_id.inner()).fetch_all(&self.pool).await
        } else {
            sqlx::query("SELECT id, provider_id, title, token_count, turn_count, session_id, template_id, model_override, created_at, updated_at FROM threads WHERE session_id=$1 ORDER BY created_at ASC")
                .bind(*session_id.inner()).fetch_all(&self.pool).await
        }.map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        rows.into_iter()
            .map(|r| self.map_thread_record(r))
            .collect()
    }
}

// Shared/global repository surfaces. These stay global in this pass.
#[async_trait]
impl TemplateRepairRepository for ArgusPostgres {
    async fn repair_placeholder_ids(&self) -> DbResult<()> {
        Ok(())
    }
}

#[async_trait]
impl AccountRepository for ArgusPostgres {
    async fn has_account(&self) -> argus_protocol::Result<bool> {
        let c: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM accounts")
            .fetch_one(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        Ok(c > 0)
    }
    async fn setup_account(
        &self,
        username: &str,
        ciphertext: &[u8],
        nonce: &[u8],
    ) -> argus_protocol::Result<()> {
        sqlx::query("INSERT INTO accounts (id, username, password, nonce, created_at, updated_at) VALUES (1,$1,$2,$3,CURRENT_TIMESTAMP::TEXT,CURRENT_TIMESTAMP::TEXT)").bind(username).bind(ciphertext).bind(nonce).execute(&self.pool).await.map_err(|e| DbError::QueryFailed{reason:e.to_string()})?;
        Ok(())
    }
    async fn configure_account(
        &self,
        username: &str,
        ciphertext: &[u8],
        nonce: &[u8],
    ) -> argus_protocol::Result<()> {
        sqlx::query("INSERT INTO accounts (id, username, password, nonce, created_at, updated_at) VALUES (1,$1,$2,$3,CURRENT_TIMESTAMP::TEXT,CURRENT_TIMESTAMP::TEXT) ON CONFLICT(id) DO UPDATE SET username=EXCLUDED.username,password=EXCLUDED.password,nonce=EXCLUDED.nonce,updated_at=CURRENT_TIMESTAMP::TEXT").bind(username).bind(ciphertext).bind(nonce).execute(&self.pool).await.map_err(|e| DbError::QueryFailed{reason:e.to_string()})?;
        Ok(())
    }
    async fn get_credentials(&self) -> argus_protocol::Result<Option<AccountCredentials>> {
        let row = sqlx::query("SELECT username,password,nonce FROM accounts WHERE id=1")
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        Ok(row.map(|r| AccountCredentials {
            username: r.get("username"),
            ciphertext: r.get("password"),
            nonce: r.get("nonce"),
        }))
    }
    async fn get_username(&self) -> argus_protocol::Result<Option<String>> {
        sqlx::query_scalar("SELECT username FROM accounts WHERE id=1")
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| {
                DbError::QueryFailed {
                    reason: e.to_string(),
                }
                .into()
            })
    }
}

#[async_trait]
impl AgentRepository for ArgusPostgres {
    async fn upsert(&self, record: &AgentRecord) -> DbResult<AgentId> {
        let tool_names = Self::serialize_json(&record.tool_names)?;
        let subagent_names = Self::serialize_json(&record.subagent_names)?;
        let thinking_config = record
            .thinking_config
            .as_ref()
            .map(Self::serialize_json)
            .transpose()?;
        let temperature = record.temperature.map(|t| (t * 100.0) as i64);
        let row = if record.id.inner() == 0 {
            sqlx::query("INSERT INTO agents (display_name,description,version,provider_id,model_id,system_prompt,tool_names,subagent_names,max_tokens,temperature,thinking_config,updated_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,CURRENT_TIMESTAMP::TEXT) ON CONFLICT(display_name) DO UPDATE SET description=EXCLUDED.description,version=EXCLUDED.version,provider_id=EXCLUDED.provider_id,model_id=EXCLUDED.model_id,system_prompt=EXCLUDED.system_prompt,tool_names=EXCLUDED.tool_names,subagent_names=EXCLUDED.subagent_names,max_tokens=EXCLUDED.max_tokens,temperature=EXCLUDED.temperature,thinking_config=EXCLUDED.thinking_config,updated_at=CURRENT_TIMESTAMP::TEXT RETURNING id")
                .bind(&record.display_name).bind(&record.description).bind(&record.version).bind(record.provider_id.map(|id| id.inner())).bind(&record.model_id).bind(&record.system_prompt).bind(&tool_names).bind(&subagent_names).bind(record.max_tokens.map(|v| v as i64)).bind(temperature).bind(&thinking_config).fetch_one(&self.pool).await
        } else {
            sqlx::query("INSERT INTO agents (id,display_name,description,version,provider_id,model_id,system_prompt,tool_names,subagent_names,max_tokens,temperature,thinking_config,updated_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,CURRENT_TIMESTAMP::TEXT) ON CONFLICT(id) DO UPDATE SET display_name=EXCLUDED.display_name,description=EXCLUDED.description,version=EXCLUDED.version,provider_id=EXCLUDED.provider_id,model_id=EXCLUDED.model_id,system_prompt=EXCLUDED.system_prompt,tool_names=EXCLUDED.tool_names,subagent_names=EXCLUDED.subagent_names,max_tokens=EXCLUDED.max_tokens,temperature=EXCLUDED.temperature,thinking_config=EXCLUDED.thinking_config,updated_at=CURRENT_TIMESTAMP::TEXT RETURNING id")
                .bind(record.id.inner()).bind(&record.display_name).bind(&record.description).bind(&record.version).bind(record.provider_id.map(|id| id.inner())).bind(&record.model_id).bind(&record.system_prompt).bind(&tool_names).bind(&subagent_names).bind(record.max_tokens.map(|v| v as i64)).bind(temperature).bind(&thinking_config).fetch_one(&self.pool).await
        }.map_err(|e| DbError::QueryFailed { reason: e.to_string() })?;
        Ok(AgentId::new(Self::get(&row, "id")?))
    }
    async fn get(&self, id: &AgentId) -> DbResult<Option<AgentRecord>> {
        let row=sqlx::query("SELECT id,display_name,description,version,provider_id,model_id,system_prompt,tool_names,subagent_names,max_tokens,temperature,thinking_config FROM agents WHERE id=$1").bind(id.inner()).fetch_optional(&self.pool).await.map_err(|e| DbError::QueryFailed{reason:e.to_string()})?;
        row.map(|r| self.map_agent_record(r)).transpose()
    }
    async fn find_by_display_name(&self, display_name: &str) -> DbResult<Option<AgentRecord>> {
        let row=sqlx::query("SELECT id,display_name,description,version,provider_id,model_id,system_prompt,tool_names,subagent_names,max_tokens,temperature,thinking_config FROM agents WHERE display_name=$1 LIMIT 1").bind(display_name).fetch_optional(&self.pool).await.map_err(|e| DbError::QueryFailed{reason:e.to_string()})?;
        row.map(|r| self.map_agent_record(r)).transpose()
    }
    async fn find_id_by_display_name(&self, display_name: &str) -> DbResult<Option<AgentId>> {
        let id: Option<i64> =
            sqlx::query_scalar("SELECT id FROM agents WHERE display_name=$1 LIMIT 1")
                .bind(display_name)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?;
        Ok(id.map(AgentId::new))
    }
    async fn list(&self) -> DbResult<Vec<AgentRecord>> {
        let rows=sqlx::query("SELECT id,display_name,description,version,provider_id,model_id,system_prompt,tool_names,subagent_names,max_tokens,temperature,thinking_config FROM agents ORDER BY display_name ASC").fetch_all(&self.pool).await.map_err(|e| DbError::QueryFailed{reason:e.to_string()})?;
        rows.into_iter().map(|r| self.map_agent_record(r)).collect()
    }
    async fn count_references(&self, id: &AgentId) -> DbResult<(i64, i64)> {
        let t: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM threads WHERE template_id=$1")
            .bind(id.inner())
            .fetch_one(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        let j: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM jobs WHERE agent_id=$1")
            .bind(id.inner())
            .fetch_one(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        Ok((t, j))
    }
    async fn delete(&self, id: &AgentId) -> DbResult<bool> {
        let r = sqlx::query("DELETE FROM agents WHERE id=$1")
            .bind(id.inner())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        Ok(r.rows_affected() > 0)
    }
}

#[async_trait]
impl LlmProviderRepository for ArgusPostgres {
    async fn upsert_provider(
        &self,
        record: &LlmProviderRecord,
    ) -> Result<LlmProviderId, ArgusError> {
        let encrypted = self
            .write_cipher
            .encrypt(record.api_key.expose_secret())
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        let models = Self::serialize_json(&record.models)?;
        let model_config = Self::serialize_json(&record.model_config)?;
        let extra_headers = Self::serialize_json(&record.extra_headers)?;
        let meta_data = Self::serialize_json(&record.meta_data)?;
        if record.is_default {
            sqlx::query("UPDATE llm_providers SET is_default=FALSE WHERE is_default=TRUE")
                .execute(&self.pool)
                .await
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?;
        }
        let row = if record.id.into_inner()==0 { sqlx::query("INSERT INTO llm_providers (kind,display_name,base_url,models,model_config,default_model,encrypted_api_key,api_key_nonce,is_default,extra_headers,meta_data,updated_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,CURRENT_TIMESTAMP::TEXT) RETURNING id")
            .bind(record.kind.as_str()).bind(&record.display_name).bind(&record.base_url).bind(&models).bind(&model_config).bind(&record.default_model).bind(&encrypted.ciphertext).bind(&encrypted.nonce).bind(record.is_default).bind(&extra_headers).bind(&meta_data).fetch_one(&self.pool).await
        } else { sqlx::query("INSERT INTO llm_providers (id,kind,display_name,base_url,models,model_config,default_model,encrypted_api_key,api_key_nonce,is_default,extra_headers,meta_data,updated_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,CURRENT_TIMESTAMP::TEXT) ON CONFLICT(id) DO UPDATE SET kind=EXCLUDED.kind,display_name=EXCLUDED.display_name,base_url=EXCLUDED.base_url,models=EXCLUDED.models,model_config=EXCLUDED.model_config,default_model=EXCLUDED.default_model,encrypted_api_key=EXCLUDED.encrypted_api_key,api_key_nonce=EXCLUDED.api_key_nonce,is_default=EXCLUDED.is_default,extra_headers=EXCLUDED.extra_headers,meta_data=EXCLUDED.meta_data,updated_at=CURRENT_TIMESTAMP::TEXT RETURNING id")
            .bind(record.id.into_inner()).bind(record.kind.as_str()).bind(&record.display_name).bind(&record.base_url).bind(&models).bind(&model_config).bind(&record.default_model).bind(&encrypted.ciphertext).bind(&encrypted.nonce).bind(record.is_default).bind(&extra_headers).bind(&meta_data).fetch_one(&self.pool).await }.map_err(|e| DbError::QueryFailed{reason:e.to_string()})?;
        Ok(LlmProviderId::new(Self::get(&row, "id")?))
    }
    async fn delete_provider(&self, id: &LlmProviderId) -> Result<bool, ArgusError> {
        let r = sqlx::query("DELETE FROM llm_providers WHERE id=$1")
            .bind(id.into_inner())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        Ok(r.rows_affected() > 0)
    }
    async fn set_default_provider(&self, id: &LlmProviderId) -> Result<(), ArgusError> {
        let mut tx = self.pool.begin().await.map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        sqlx::query("UPDATE llm_providers SET is_default=FALSE WHERE is_default=TRUE")
            .execute(&mut *tx)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        let r=sqlx::query("UPDATE llm_providers SET is_default=TRUE,updated_at=CURRENT_TIMESTAMP::TEXT WHERE id=$1").bind(id.into_inner()).execute(&mut *tx).await.map_err(|e| DbError::QueryFailed{reason:e.to_string()})?;
        tx.commit().await.map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        if r.rows_affected() == 0 {
            Err(DbError::NotFound { id: id.to_string() }.into())
        } else {
            Ok(())
        }
    }
    async fn get_provider(
        &self,
        id: &LlmProviderId,
    ) -> Result<Option<LlmProviderRecord>, ArgusError> {
        let row=sqlx::query("SELECT id,kind,display_name,base_url,models,model_config,default_model,encrypted_api_key,api_key_nonce,is_default,extra_headers,meta_data FROM llm_providers WHERE id=$1").bind(id.into_inner()).fetch_optional(&self.pool).await.map_err(|e| DbError::QueryFailed{reason:e.to_string()})?;
        row.map(|r| self.map_llm_record(r)).transpose()
    }
    async fn list_providers(&self) -> Result<Vec<LlmProviderRecord>, ArgusError> {
        let rows=sqlx::query("SELECT id,kind,display_name,base_url,models,model_config,default_model,encrypted_api_key,api_key_nonce,is_default,extra_headers,meta_data FROM llm_providers ORDER BY display_name ASC").fetch_all(&self.pool).await.map_err(|e| DbError::QueryFailed{reason:e.to_string()})?;
        rows.into_iter().map(|r| self.map_llm_record(r)).collect()
    }
    async fn get_default_provider(&self) -> Result<Option<LlmProviderRecord>, ArgusError> {
        let row=sqlx::query("SELECT id,kind,display_name,base_url,models,model_config,default_model,encrypted_api_key,api_key_nonce,is_default,extra_headers,meta_data FROM llm_providers WHERE is_default=TRUE LIMIT 1").fetch_optional(&self.pool).await.map_err(|e| DbError::QueryFailed{reason:e.to_string()})?;
        row.map(|r| self.map_llm_record(r)).transpose()
    }
    async fn get_default_provider_id(&self) -> Result<Option<LlmProviderId>, ArgusError> {
        let id: Option<i64> =
            sqlx::query_scalar("SELECT id FROM llm_providers WHERE is_default=TRUE LIMIT 1")
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| DbError::QueryFailed {
                    reason: e.to_string(),
                })?;
        Ok(id.map(LlmProviderId::new))
    }
}

#[async_trait]
impl AgentRunRepository for ArgusPostgres {
    async fn insert_agent_run(&self, record: &AgentRunRecord) -> DbResult<()> {
        sqlx::query("INSERT INTO agent_runs (id,agent_id,session_id,thread_id,prompt,status,result,error,created_at,updated_at,completed_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)").bind(record.id.0).bind(record.agent_id.inner()).bind(*record.session_id.inner()).bind(*record.thread_id.inner()).bind(&record.prompt).bind(record.status.as_str()).bind(&record.result).bind(&record.error).bind(&record.created_at).bind(&record.updated_at).bind(&record.completed_at).execute(&self.pool).await.map_err(|e| DbError::QueryFailed{reason:e.to_string()})?;
        Ok(())
    }
    async fn get_agent_run(&self, id: &AgentRunId) -> DbResult<Option<AgentRunRecord>> {
        let row=sqlx::query("SELECT id,agent_id,session_id,thread_id,prompt,status,result,error,created_at,updated_at,completed_at FROM agent_runs WHERE id=$1").bind(id.0).fetch_optional(&self.pool).await.map_err(|e| DbError::QueryFailed{reason:e.to_string()})?;
        row.map(|r| {
            let status_text: String = Self::get(&r, "status")?;
            Ok(AgentRunRecord {
                id: AgentRunId(Self::get(&r, "id")?),
                agent_id: AgentId::new(Self::get(&r, "agent_id")?),
                session_id: SessionId(Self::get(&r, "session_id")?),
                thread_id: ThreadId(Self::get(&r, "thread_id")?),
                prompt: Self::get(&r, "prompt")?,
                status: AgentRunStatus::parse(&status_text).ok_or_else(|| {
                    DbError::QueryFailed {
                        reason: format!("invalid agent run status: {status_text}"),
                    }
                })?,
                result: Self::get(&r, "result")?,
                error: Self::get(&r, "error")?,
                created_at: Self::get(&r, "created_at")?,
                updated_at: Self::get(&r, "updated_at")?,
                completed_at: Self::get(&r, "completed_at")?,
            })
        })
        .transpose()
    }
    async fn update_agent_run_status(
        &self,
        id: &AgentRunId,
        status: AgentRunStatus,
        result: Option<&str>,
        error: Option<&str>,
        completed_at: Option<&str>,
        updated_at: &str,
    ) -> DbResult<()> {
        sqlx::query("UPDATE agent_runs SET status=$1,result=$2,error=$3,completed_at=$4,updated_at=$5 WHERE id=$6").bind(status.as_str()).bind(result).bind(error).bind(completed_at).bind(updated_at).bind(id.0).execute(&self.pool).await.map_err(|e| DbError::QueryFailed{reason:e.to_string()})?;
        Ok(())
    }
    async fn delete_agent_run(&self, id: &AgentRunId) -> DbResult<bool> {
        let r = sqlx::query("DELETE FROM agent_runs WHERE id=$1")
            .bind(id.0)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        Ok(r.rows_affected() > 0)
    }
}

#[async_trait]
impl McpRepository for ArgusPostgres {
    async fn upsert_mcp_server(&self, record: &McpServerRecord) -> DbResult<i64> {
        let transport = Self::serialize_json(&record.transport)?;
        let status = Self::mcp_status_text(record.status);
        let row=if let Some(id)=record.id{sqlx::query("INSERT INTO mcp_servers (id,display_name,enabled,transport,timeout_ms,status,last_checked_at,last_success_at,last_error,discovered_tool_count,updated_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,CURRENT_TIMESTAMP::TEXT) ON CONFLICT(id) DO UPDATE SET display_name=EXCLUDED.display_name,enabled=EXCLUDED.enabled,transport=EXCLUDED.transport,timeout_ms=EXCLUDED.timeout_ms,status=EXCLUDED.status,last_checked_at=EXCLUDED.last_checked_at,last_success_at=EXCLUDED.last_success_at,last_error=EXCLUDED.last_error,discovered_tool_count=EXCLUDED.discovered_tool_count,updated_at=CURRENT_TIMESTAMP::TEXT RETURNING id").bind(id).bind(&record.display_name).bind(record.enabled).bind(&transport).bind(record.timeout_ms as i64).bind(status).bind(&record.last_checked_at).bind(&record.last_success_at).bind(&record.last_error).bind(record.discovered_tool_count as i64).fetch_one(&self.pool).await}else{sqlx::query("INSERT INTO mcp_servers (display_name,enabled,transport,timeout_ms,status,last_checked_at,last_success_at,last_error,discovered_tool_count,updated_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,CURRENT_TIMESTAMP::TEXT) RETURNING id").bind(&record.display_name).bind(record.enabled).bind(&transport).bind(record.timeout_ms as i64).bind(status).bind(&record.last_checked_at).bind(&record.last_success_at).bind(&record.last_error).bind(record.discovered_tool_count as i64).fetch_one(&self.pool).await}.map_err(|e|DbError::QueryFailed{reason:e.to_string()})?;
        Self::get(&row, "id")
    }
    async fn get_mcp_server(&self, id: i64) -> DbResult<Option<McpServerRecord>> {
        let row=sqlx::query("SELECT id,display_name,enabled,transport,timeout_ms,status,last_checked_at,last_success_at,last_error,discovered_tool_count FROM mcp_servers WHERE id=$1").bind(id).fetch_optional(&self.pool).await.map_err(|e|DbError::QueryFailed{reason:e.to_string()})?;
        row.map(|r| self.map_mcp_server(r)).transpose()
    }
    async fn list_mcp_servers(&self) -> DbResult<Vec<McpServerRecord>> {
        let rows=sqlx::query("SELECT id,display_name,enabled,transport,timeout_ms,status,last_checked_at,last_success_at,last_error,discovered_tool_count FROM mcp_servers ORDER BY display_name ASC").fetch_all(&self.pool).await.map_err(|e|DbError::QueryFailed{reason:e.to_string()})?;
        rows.into_iter().map(|r| self.map_mcp_server(r)).collect()
    }
    async fn delete_mcp_server(&self, id: i64) -> DbResult<bool> {
        let r = sqlx::query("DELETE FROM mcp_servers WHERE id=$1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        Ok(r.rows_affected() > 0)
    }
    async fn replace_mcp_server_tools(
        &self,
        server_id: i64,
        tools: &[McpDiscoveredToolRecord],
    ) -> DbResult<()> {
        let mut tx = self.pool.begin().await.map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        sqlx::query("DELETE FROM mcp_discovered_tools WHERE server_id=$1")
            .bind(server_id)
            .execute(&mut *tx)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        for t in tools {
            sqlx::query("INSERT INTO mcp_discovered_tools (server_id,tool_name_original,description,schema_json,annotations_json) VALUES ($1,$2,$3,$4,$5)").bind(server_id).bind(&t.tool_name_original).bind(&t.description).bind(t.schema.to_string()).bind(t.annotations.as_ref().map(|v|v.to_string())).execute(&mut *tx).await.map_err(|e|DbError::QueryFailed{reason:e.to_string()})?;
        }
        tx.commit().await.map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        Ok(())
    }
    async fn list_mcp_server_tools(
        &self,
        server_id: i64,
    ) -> DbResult<Vec<McpDiscoveredToolRecord>> {
        let rows=sqlx::query("SELECT server_id,tool_name_original,description,schema_json,annotations_json FROM mcp_discovered_tools WHERE server_id=$1 ORDER BY tool_name_original ASC").bind(server_id).fetch_all(&self.pool).await.map_err(|e|DbError::QueryFailed{reason:e.to_string()})?;
        rows.into_iter()
            .map(|r| {
                Ok(McpDiscoveredToolRecord {
                    server_id: Self::get(&r, "server_id")?,
                    tool_name_original: Self::get(&r, "tool_name_original")?,
                    description: Self::get(&r, "description")?,
                    schema: serde_json::from_str(&Self::get::<String>(&r, "schema_json")?)
                        .map_err(|e| DbError::QueryFailed {
                            reason: e.to_string(),
                        })?,
                    annotations: Self::get::<Option<String>>(&r, "annotations_json")?
                        .map(|s| serde_json::from_str(&s))
                        .transpose()
                        .map_err(|e| DbError::QueryFailed {
                            reason: e.to_string(),
                        })?,
                })
            })
            .collect()
    }
    async fn set_agent_mcp_bindings(
        &self,
        agent_id: AgentId,
        bindings: &[AgentMcpBinding],
    ) -> DbResult<()> {
        let mut tx = self.pool.begin().await.map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        sqlx::query("DELETE FROM agent_mcp_bindings WHERE agent_id=$1")
            .bind(agent_id.inner())
            .execute(&mut *tx)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        for b in bindings {
            sqlx::query("INSERT INTO agent_mcp_bindings (agent_id,server_id,allowed_tools) VALUES ($1,$2,$3)").bind(agent_id.inner()).bind(b.server.server_id).bind(b.allowed_tools.as_ref().map(Self::serialize_json).transpose()?).execute(&mut *tx).await.map_err(|e|DbError::QueryFailed{reason:e.to_string()})?;
        }
        tx.commit().await.map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        Ok(())
    }
    async fn list_agent_mcp_bindings(&self, agent_id: AgentId) -> DbResult<Vec<AgentMcpBinding>> {
        let rows=sqlx::query("SELECT agent_id,server_id,allowed_tools FROM agent_mcp_bindings WHERE agent_id=$1 ORDER BY server_id ASC").bind(agent_id.inner()).fetch_all(&self.pool).await.map_err(|e|DbError::QueryFailed{reason:e.to_string()})?;
        rows.into_iter()
            .map(|r| {
                Ok(AgentMcpBinding {
                    server: AgentMcpServerBinding {
                        agent_id: AgentId::new(Self::get(&r, "agent_id")?),
                        server_id: Self::get(&r, "server_id")?,
                    },
                    allowed_tools: Self::get::<Option<String>>(&r, "allowed_tools")?
                        .map(|s| serde_json::from_str(&s))
                        .transpose()
                        .map_err(|e| DbError::QueryFailed {
                            reason: e.to_string(),
                        })?,
                })
            })
            .collect()
    }
}

#[async_trait]
impl JobRepository for ArgusPostgres {
    async fn create(&self, job: &JobRecord) -> DbResult<()> {
        let depends = Self::serialize_json(
            &job.depends_on
                .iter()
                .map(|id| id.to_string())
                .collect::<Vec<_>>(),
        )?;
        sqlx::query("INSERT INTO jobs (id,job_type,name,status,agent_id,context,prompt,thread_id,group_id,depends_on,cron_expr,scheduled_at,started_at,finished_at,parent_job_id,result) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)").bind(job.id.to_string()).bind(job.job_type.as_str()).bind(&job.name).bind(job.status.as_str()).bind(job.agent_id.inner()).bind(&job.context).bind(&job.prompt).bind(job.thread_id.map(|id|*id.inner())).bind(&job.group_id).bind(&depends).bind(&job.cron_expr).bind(&job.scheduled_at).bind(&job.started_at).bind(&job.finished_at).bind(job.parent_job_id.as_ref().map(|id|id.to_string())).bind(job.result.as_ref().map(Self::serialize_json).transpose()?).execute(&self.pool).await.map_err(|e|DbError::QueryFailed{reason:e.to_string()})?;
        Ok(())
    }
    async fn get(&self, id: &JobId) -> DbResult<Option<JobRecord>> {
        let row=sqlx::query("SELECT id,job_type,name,status,agent_id,context,prompt,thread_id,group_id,depends_on,cron_expr,scheduled_at,started_at,finished_at,parent_job_id,result FROM jobs WHERE id=$1").bind(id.to_string()).fetch_optional(&self.pool).await.map_err(|e|DbError::QueryFailed{reason:e.to_string()})?;
        row.map(|r| self.map_job_record(r)).transpose()
    }
    async fn update_status(
        &self,
        id: &JobId,
        status: JobStatus,
        started_at: Option<&str>,
        finished_at: Option<&str>,
    ) -> DbResult<()> {
        sqlx::query("UPDATE jobs SET status=$1,started_at=$2,finished_at=$3,updated_at=CURRENT_TIMESTAMP::TEXT WHERE id=$4 AND status NOT IN ('succeeded','failed','cancelled')").bind(status.as_str()).bind(started_at).bind(finished_at).bind(id.to_string()).execute(&self.pool).await.map_err(|e|DbError::QueryFailed{reason:e.to_string()})?;
        Ok(())
    }
    async fn update_result(&self, id: &JobId, result: &JobResult) -> DbResult<()> {
        sqlx::query("UPDATE jobs SET result=$1,updated_at=CURRENT_TIMESTAMP::TEXT WHERE id=$2")
            .bind(Self::serialize_json(result)?)
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        Ok(())
    }
    async fn update_thread_id(&self, id: &JobId, thread_id: &ThreadId) -> DbResult<()> {
        sqlx::query("UPDATE jobs SET thread_id=$1,updated_at=CURRENT_TIMESTAMP::TEXT WHERE id=$2")
            .bind(*thread_id.inner())
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        Ok(())
    }
    async fn find_ready_jobs(&self, limit: usize) -> DbResult<Vec<JobRecord>> {
        let rows=sqlx::query("SELECT id,job_type,name,status,agent_id,context,prompt,thread_id,group_id,depends_on,cron_expr,scheduled_at,started_at,finished_at,parent_job_id,result FROM jobs WHERE status='pending' AND job_type!='cron' ORDER BY created_at ASC LIMIT $1").bind(limit as i64).fetch_all(&self.pool).await.map_err(|e|DbError::QueryFailed{reason:e.to_string()})?;
        rows.into_iter().map(|r| self.map_job_record(r)).collect()
    }
    async fn find_due_cron_jobs(&self, now: &str) -> DbResult<Vec<JobRecord>> {
        let rows=sqlx::query("SELECT id,job_type,name,status,agent_id,context,prompt,thread_id,group_id,depends_on,cron_expr,scheduled_at,started_at,finished_at,parent_job_id,result FROM jobs WHERE job_type='cron' AND scheduled_at IS NOT NULL AND scheduled_at <= $1 ORDER BY scheduled_at ASC").bind(now).fetch_all(&self.pool).await.map_err(|e|DbError::QueryFailed{reason:e.to_string()})?;
        rows.into_iter().map(|r| self.map_job_record(r)).collect()
    }
    async fn update_scheduled_at(&self, id: &JobId, next: &str) -> DbResult<()> {
        sqlx::query(
            "UPDATE jobs SET scheduled_at=$1,updated_at=CURRENT_TIMESTAMP::TEXT WHERE id=$2",
        )
        .bind(next)
        .bind(id.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| DbError::QueryFailed {
            reason: e.to_string(),
        })?;
        Ok(())
    }
    async fn list_by_group(&self, group_id: &str) -> DbResult<Vec<JobRecord>> {
        let rows=sqlx::query("SELECT id,job_type,name,status,agent_id,context,prompt,thread_id,group_id,depends_on,cron_expr,scheduled_at,started_at,finished_at,parent_job_id,result FROM jobs WHERE group_id=$1 ORDER BY created_at ASC").bind(group_id).fetch_all(&self.pool).await.map_err(|e|DbError::QueryFailed{reason:e.to_string()})?;
        rows.into_iter().map(|r| self.map_job_record(r)).collect()
    }
    async fn delete(&self, id: &JobId) -> DbResult<bool> {
        let r = sqlx::query("DELETE FROM jobs WHERE id=$1")
            .bind(id.to_string())
            .execute(&self.pool)
            .await
            .map_err(|e| DbError::QueryFailed {
                reason: e.to_string(),
            })?;
        Ok(r.rows_affected() > 0)
    }
}

impl ArgusPostgres {
    fn map_job_record(&self, row: PgRow) -> DbResult<JobRecord> {
        let depends_on =
            serde_json::from_str::<Vec<String>>(&Self::get::<String>(&row, "depends_on")?)
                .map(|ids| ids.into_iter().map(JobId::new).collect())
                .unwrap_or_default();
        let thread_id = Self::get::<Option<Uuid>>(&row, "thread_id")?.map(ThreadId);
        let parent_job_id = Self::get::<Option<String>>(&row, "parent_job_id")?.map(JobId::new);
        let result = Self::get::<Option<String>>(&row, "result")?
            .and_then(|s| serde_json::from_str(&s).ok());
        Ok(JobRecord {
            id: JobId::new(Self::get::<String>(&row, "id")?),
            job_type: JobType::parse_str(&Self::get::<String>(&row, "job_type")?)
                .map_err(|e| DbError::QueryFailed { reason: e })?,
            name: Self::get(&row, "name")?,
            status: JobStatus::parse_str(&Self::get::<String>(&row, "status")?)
                .map_err(|e| DbError::QueryFailed { reason: e })?,
            agent_id: AgentId::new(Self::get(&row, "agent_id")?),
            context: Self::get(&row, "context")?,
            prompt: Self::get(&row, "prompt")?,
            thread_id,
            group_id: Self::get(&row, "group_id")?,
            depends_on,
            cron_expr: Self::get(&row, "cron_expr")?,
            scheduled_at: Self::get(&row, "scheduled_at")?,
            started_at: Self::get(&row, "started_at")?,
            finished_at: Self::get(&row, "finished_at")?,
            parent_job_id,
            result,
        })
    }
}
