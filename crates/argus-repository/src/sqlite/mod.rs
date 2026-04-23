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
mod admin_settings;
mod agent;
mod job;
mod llm_provider;
mod mcp;
mod session;
mod thread;

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
            subagent_names: String,
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
                    tool_names, subagent_names, max_tokens, temperature, thinking_config
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
            subagent_names,
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
                                 system_prompt, tool_names, subagent_names, max_tokens, temperature, thinking_config)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT(display_name) DO NOTHING",
        )
        .bind(&display_name)
        .bind(&description)
        .bind(&version)
        .bind(provider_id)
        .bind(&model_id)
        .bind(&system_prompt)
        .bind(&tool_names)
        .bind(&subagent_names)
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
            INSERT INTO agents (id, display_name, description, version, provider_id, system_prompt, tool_names, subagent_names, max_tokens, temperature, created_at, updated_at)
            VALUES (0, 'Legacy Zero Agent', 'legacy', '1.0.0', NULL, 'prompt', '[]', '["Chrome Explore"]', NULL, NULL, datetime('now'), datetime('now'))
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
    use sqlx::Row;

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

    #[tokio::test]
    async fn migrate_does_not_create_legacy_workflows_table() {
        let pool = super::connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite should connect");
        super::migrate(&pool)
            .await
            .expect("repository migrations should succeed");

        let rows = sqlx::query(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name IN ('jobs', 'workflows')",
        )
        .fetch_all(&pool)
        .await
        .expect("schema query should succeed");

        let table_names: Vec<String> = rows
            .into_iter()
            .map(|row| row.get::<String, _>("name"))
            .collect();

        assert!(
            table_names.iter().any(|name| name == "jobs"),
            "job table should still exist after migrations"
        );
        assert!(
            table_names.iter().all(|name| name != "workflows"),
            "legacy workflows table should no longer be created"
        );
    }

    #[tokio::test]
    async fn flatten_subagent_migration_preserves_parent_child_bindings() {
        let pool = super::connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite should connect");

        sqlx::raw_sql(
            r#"
            CREATE TABLE llm_providers (
                id INTEGER PRIMARY KEY AUTOINCREMENT
            );

            CREATE TABLE agents (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                display_name TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                version TEXT NOT NULL DEFAULT '1.0.0',
                provider_id INTEGER REFERENCES llm_providers(id) ON DELETE RESTRICT,
                model_id TEXT,
                system_prompt TEXT NOT NULL,
                tool_names TEXT NOT NULL DEFAULT '[]',
                max_tokens INTEGER,
                temperature INTEGER,
                parent_agent_id INTEGER REFERENCES agents(id),
                agent_type TEXT NOT NULL DEFAULT 'standard' CHECK(agent_type IN ('standard', 'subagent')),
                thinking_config TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );
            "#,
        )
        .execute(&pool)
        .await
        .expect("legacy schema should be created");

        sqlx::query(
            r#"
            INSERT INTO agents (id, display_name, description, version, provider_id, model_id, system_prompt, tool_names, max_tokens, temperature, parent_agent_id, agent_type, thinking_config)
            VALUES
                (1, 'Parent', 'parent', '1.0.0', NULL, NULL, 'parent prompt', '[]', NULL, NULL, NULL, 'standard', NULL),
                (2, 'Child A', 'child', '1.0.0', NULL, NULL, 'child prompt', '[]', NULL, NULL, 1, 'subagent', NULL),
                (3, 'Child B', 'child', '1.0.0', NULL, NULL, 'child prompt', '[]', NULL, NULL, 1, 'subagent', NULL)
            "#,
        )
        .execute(&pool)
        .await
        .expect("legacy agents should insert");

        let migration_sql = std::fs::read_to_string(format!(
            "{}/migrations/20260411000000_flatten_subagent_persistence.sql",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("flatten-subagent migration should exist");

        sqlx::raw_sql(&migration_sql)
            .execute(&pool)
            .await
            .expect("flatten-subagent migration should run");

        let parent_subagent_names: String =
            sqlx::query_scalar("SELECT subagent_names FROM agents WHERE id = 1")
                .fetch_one(&pool)
                .await
                .expect("parent should still exist");
        let child_subagent_names: String =
            sqlx::query_scalar("SELECT subagent_names FROM agents WHERE id = 2")
                .fetch_one(&pool)
                .await
                .expect("child should still exist");

        assert_eq!(parent_subagent_names, r#"["Child A","Child B"]"#);
        assert_eq!(child_subagent_names, "[]");
    }

    #[tokio::test]
    async fn flatten_subagent_migration_succeeds_with_existing_agent_foreign_keys() {
        let pool = super::connect("sqlite::memory:")
            .await
            .expect("in-memory sqlite should connect");

        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .expect("foreign keys should be enabled");

        sqlx::raw_sql(
            r#"
            CREATE TABLE llm_providers (
                id INTEGER PRIMARY KEY AUTOINCREMENT
            );

            CREATE TABLE agents (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                display_name TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                version TEXT NOT NULL DEFAULT '1.0.0',
                provider_id INTEGER REFERENCES llm_providers(id) ON DELETE RESTRICT,
                model_id TEXT,
                system_prompt TEXT NOT NULL,
                tool_names TEXT NOT NULL DEFAULT '[]',
                max_tokens INTEGER,
                temperature INTEGER,
                parent_agent_id INTEGER REFERENCES agents(id),
                agent_type TEXT NOT NULL DEFAULT 'standard' CHECK(agent_type IN ('standard', 'subagent')),
                thinking_config TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                provider_id INTEGER NOT NULL REFERENCES llm_providers(id) ON DELETE RESTRICT,
                title TEXT,
                token_count INTEGER NOT NULL DEFAULT 0,
                turn_count INTEGER NOT NULL DEFAULT 0,
                session_id INTEGER,
                template_id INTEGER REFERENCES agents(id),
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE jobs (
                id TEXT PRIMARY KEY NOT NULL,
                job_type TEXT NOT NULL DEFAULT 'standalone',
                name TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                agent_id INTEGER NOT NULL REFERENCES agents(id) ON DELETE RESTRICT,
                context TEXT,
                prompt TEXT NOT NULL,
                thread_id TEXT,
                group_id TEXT,
                depends_on TEXT NOT NULL DEFAULT '[]',
                cron_expr TEXT,
                scheduled_at TEXT,
                started_at TEXT,
                finished_at TEXT,
                parent_job_id TEXT,
                result TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE mcp_servers (
                id INTEGER PRIMARY KEY AUTOINCREMENT
            );

            CREATE TABLE agent_mcp_servers (
                agent_id INTEGER NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
                server_id INTEGER NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
                use_tool_whitelist INTEGER NOT NULL DEFAULT 0 CHECK (use_tool_whitelist IN (0, 1)),
                PRIMARY KEY (agent_id, server_id)
            );
            "#,
        )
        .execute(&pool)
        .await
        .expect("legacy schema with agent foreign keys should be created");

        sqlx::query("INSERT INTO llm_providers (id) VALUES (1)")
            .execute(&pool)
            .await
            .expect("provider should insert");
        sqlx::query("INSERT INTO mcp_servers (id) VALUES (1)")
            .execute(&pool)
            .await
            .expect("mcp server should insert");

        sqlx::query(
            r#"
            INSERT INTO agents (id, display_name, description, version, provider_id, model_id, system_prompt, tool_names, max_tokens, temperature, parent_agent_id, agent_type, thinking_config)
            VALUES
                (1, 'Parent', 'parent', '1.0.0', 1, NULL, 'parent prompt', '[]', NULL, NULL, NULL, 'standard', NULL),
                (2, 'Child', 'child', '1.0.0', 1, NULL, 'child prompt', '[]', NULL, NULL, 1, 'subagent', NULL)
            "#,
        )
        .execute(&pool)
        .await
        .expect("legacy agents should insert");

        sqlx::query("INSERT INTO threads (id, provider_id, template_id) VALUES ('thread-1', 1, 1)")
            .execute(&pool)
            .await
            .expect("thread should insert");
        sqlx::query(
            "INSERT INTO jobs (id, name, agent_id, prompt) VALUES ('job-1', 'Job', 2, 'work')",
        )
        .execute(&pool)
        .await
        .expect("job should insert");
        sqlx::query(
            "INSERT INTO agent_mcp_servers (agent_id, server_id, use_tool_whitelist) VALUES (1, 1, 0)",
        )
        .execute(&pool)
        .await
        .expect("agent mcp binding should insert");

        let migration_sql = std::fs::read_to_string(format!(
            "{}/migrations/20260411000000_flatten_subagent_persistence.sql",
            env!("CARGO_MANIFEST_DIR")
        ))
        .expect("flatten-subagent migration should exist");

        sqlx::raw_sql(&migration_sql)
            .execute(&pool)
            .await
            .expect("flatten-subagent migration should preserve foreign-keyed agent rows");

        let thread_template_id: i64 =
            sqlx::query_scalar("SELECT template_id FROM threads WHERE id = 'thread-1'")
                .fetch_one(&pool)
                .await
                .expect("thread should still reference an agent");
        let job_agent_id: i64 = sqlx::query_scalar("SELECT agent_id FROM jobs WHERE id = 'job-1'")
            .fetch_one(&pool)
            .await
            .expect("job should still reference an agent");
        let binding_count: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM agent_mcp_servers WHERE agent_id = 1")
                .fetch_one(&pool)
                .await
                .expect("mcp binding should still reference the agent");

        assert_eq!(thread_template_id, 1);
        assert_eq!(job_agent_id, 2);
        assert_eq!(binding_count, 1);
    }

    #[tokio::test]
    async fn flatten_subagent_migration_via_sqlx_migrator_succeeds_with_existing_agent_foreign_keys()
     {
        let db_path = std::env::temp_dir().join(format!(
            "argus-repository-migrate-{}.sqlite",
            uuid::Uuid::new_v4()
        ));
        let pool = super::connect_path(&db_path)
            .await
            .expect("sqlite file should connect");

        sqlx::query("PRAGMA foreign_keys = ON")
            .execute(&pool)
            .await
            .expect("foreign keys should be enabled");

        sqlx::raw_sql(
            r#"
            CREATE TABLE llm_providers (
                id INTEGER PRIMARY KEY AUTOINCREMENT
            );

            CREATE TABLE agents (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                display_name TEXT NOT NULL,
                description TEXT NOT NULL DEFAULT '',
                version TEXT NOT NULL DEFAULT '1.0.0',
                provider_id INTEGER REFERENCES llm_providers(id) ON DELETE RESTRICT,
                model_id TEXT,
                system_prompt TEXT NOT NULL,
                tool_names TEXT NOT NULL DEFAULT '[]',
                max_tokens INTEGER,
                temperature INTEGER,
                parent_agent_id INTEGER REFERENCES agents(id),
                agent_type TEXT NOT NULL DEFAULT 'standard' CHECK(agent_type IN ('standard', 'subagent')),
                thinking_config TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE threads (
                id TEXT PRIMARY KEY,
                provider_id INTEGER NOT NULL REFERENCES llm_providers(id) ON DELETE RESTRICT,
                title TEXT,
                token_count INTEGER NOT NULL DEFAULT 0,
                turn_count INTEGER NOT NULL DEFAULT 0,
                session_id INTEGER,
                template_id INTEGER REFERENCES agents(id),
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE jobs (
                id TEXT PRIMARY KEY NOT NULL,
                job_type TEXT NOT NULL DEFAULT 'standalone',
                name TEXT NOT NULL,
                status TEXT NOT NULL DEFAULT 'pending',
                agent_id INTEGER NOT NULL REFERENCES agents(id) ON DELETE RESTRICT,
                context TEXT,
                prompt TEXT NOT NULL,
                thread_id TEXT,
                group_id TEXT,
                depends_on TEXT NOT NULL DEFAULT '[]',
                cron_expr TEXT,
                scheduled_at TEXT,
                started_at TEXT,
                finished_at TEXT,
                parent_job_id TEXT,
                result TEXT,
                created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
                updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE mcp_servers (
                id INTEGER PRIMARY KEY AUTOINCREMENT
            );

            CREATE TABLE agent_mcp_servers (
                agent_id INTEGER NOT NULL REFERENCES agents(id) ON DELETE CASCADE,
                server_id INTEGER NOT NULL REFERENCES mcp_servers(id) ON DELETE CASCADE,
                use_tool_whitelist INTEGER NOT NULL DEFAULT 0 CHECK (use_tool_whitelist IN (0, 1)),
                PRIMARY KEY (agent_id, server_id)
            );

            CREATE TABLE _sqlx_migrations (
                version BIGINT PRIMARY KEY,
                description TEXT NOT NULL,
                installed_on TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
                success BOOLEAN NOT NULL,
                checksum BLOB NOT NULL,
                execution_time BIGINT NOT NULL
            );
            "#,
        )
        .execute(&pool)
        .await
        .expect("legacy schema with migrations table should be created");

        let migrator = sqlx::migrate::Migrator::new(std::path::Path::new(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/migrations"
        )))
        .await
        .expect("migrator should load repository migrations");

        for migration in migrator
            .iter()
            .filter(|migration| migration.version < 20260411000000)
        {
            sqlx::query(
                "INSERT INTO _sqlx_migrations (version, description, success, checksum, execution_time)
                 VALUES (?1, ?2, 1, ?3, 0)",
            )
            .bind(migration.version)
            .bind(migration.description.as_ref())
            .bind(migration.checksum.as_ref())
            .execute(&pool)
            .await
            .expect("previous migration record should insert");
        }

        sqlx::query("INSERT INTO llm_providers (id) VALUES (1)")
            .execute(&pool)
            .await
            .expect("provider should insert");
        sqlx::query("INSERT INTO mcp_servers (id) VALUES (1)")
            .execute(&pool)
            .await
            .expect("mcp server should insert");

        sqlx::query(
            r#"
            INSERT INTO agents (id, display_name, description, version, provider_id, model_id, system_prompt, tool_names, max_tokens, temperature, parent_agent_id, agent_type, thinking_config)
            VALUES
                (1, 'Parent', 'parent', '1.0.0', 1, NULL, 'parent prompt', '[]', NULL, NULL, NULL, 'standard', NULL),
                (2, 'Child', 'child', '1.0.0', 1, NULL, 'child prompt', '[]', NULL, NULL, 1, 'subagent', NULL)
            "#,
        )
        .execute(&pool)
        .await
        .expect("legacy agents should insert");

        sqlx::query("INSERT INTO threads (id, provider_id, template_id) VALUES ('thread-1', 1, 1)")
            .execute(&pool)
            .await
            .expect("thread should insert");
        sqlx::query(
            "INSERT INTO jobs (id, name, agent_id, prompt) VALUES ('job-1', 'Job', 2, 'work')",
        )
        .execute(&pool)
        .await
        .expect("job should insert");
        sqlx::query(
            "INSERT INTO agent_mcp_servers (agent_id, server_id, use_tool_whitelist) VALUES (1, 1, 0)",
        )
        .execute(&pool)
        .await
        .expect("agent mcp binding should insert");

        let migrate_result = super::migrate(&pool).await;
        if migrate_result.is_ok() {
            let parent_subagent_names: String =
                sqlx::query_scalar("SELECT subagent_names FROM agents WHERE id = 1")
                    .fetch_one(&pool)
                    .await
                    .expect("parent should still exist after migrator run");
            let thread_template_id: i64 =
                sqlx::query_scalar("SELECT template_id FROM threads WHERE id = 'thread-1'")
                    .fetch_one(&pool)
                    .await
                    .expect("thread should still reference an agent after migrator run");
            let job_agent_id: i64 =
                sqlx::query_scalar("SELECT agent_id FROM jobs WHERE id = 'job-1'")
                    .fetch_one(&pool)
                    .await
                    .expect("job should still reference an agent after migrator run");

            assert_eq!(parent_subagent_names, r#"["Child"]"#);
            assert_eq!(thread_template_id, 1);
            assert_eq!(job_agent_id, 2);
        }
        if db_path.exists() {
            let _ = std::fs::remove_file(&db_path);
        }
        migrate_result.expect("sqlx migrator should apply flatten migration successfully");
    }
}
