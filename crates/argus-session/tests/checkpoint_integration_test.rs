//! Integration tests for checkpoint and history persistence functionality.

use std::sync::Arc;

use argus_protocol::{
    AgentId, ProviderId, Result, SessionId, ThreadId,
};
use argus_session::SessionManager;
use argus_template::TemplateManager;
use argus_thread::CompactorManager;
use argus_tool::ToolManager;
use chrono::Utc;
use sqlx::{Row, SqlitePool};

/// Test helper to create an in-memory database for testing.
async fn create_test_pool() -> SqlitePool {
    let pool = SqlitePool::connect(":memory:").await.unwrap();

    // Run migrations
    sqlx::query(
        r#"
        -- LLM Providers table
        CREATE TABLE llm_providers (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            kind TEXT NOT NULL,
            display_name TEXT NOT NULL,
            base_url TEXT NOT NULL,
            models TEXT NOT NULL DEFAULT '[]',
            default_model TEXT NOT NULL,
            encrypted_api_key BLOB NOT NULL,
            api_key_nonce BLOB NOT NULL,
            extra_headers TEXT NOT NULL DEFAULT '{}',
            is_default INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        -- Agents table
        CREATE TABLE agents (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            display_name TEXT NOT NULL,
            description TEXT NOT NULL DEFAULT '',
            version TEXT NOT NULL DEFAULT '1.0.0',
            provider_id INTEGER REFERENCES llm_providers(id) ON DELETE RESTRICT,
            system_prompt TEXT NOT NULL,
            tool_names TEXT NOT NULL DEFAULT '[]',
            max_tokens INTEGER,
            temperature INTEGER,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        -- Sessions table
        CREATE TABLE sessions (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        -- Threads table
        CREATE TABLE threads (
            id TEXT PRIMARY KEY,
            provider_id INTEGER NOT NULL REFERENCES llm_providers(id) ON DELETE RESTRICT,
            session_id INTEGER REFERENCES sessions(id),
            template_id INTEGER REFERENCES agents(id),
            title TEXT,
            token_count INTEGER NOT NULL DEFAULT 0,
            turn_count INTEGER NOT NULL DEFAULT 0,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP,
            updated_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        -- Messages table
        CREATE TABLE messages (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
            seq INTEGER NOT NULL,
            turn_seq INTEGER NOT NULL DEFAULT 0,
            role TEXT NOT NULL,
            content TEXT NOT NULL,
            tool_call_id TEXT,
            tool_name TEXT,
            tool_calls TEXT,
            created_at TEXT NOT NULL DEFAULT CURRENT_TIMESTAMP
        );

        CREATE INDEX idx_messages_thread_id ON messages(thread_id);
        CREATE INDEX idx_messages_thread_seq ON messages(thread_id, seq);
        CREATE INDEX idx_messages_thread_turn ON messages(thread_id, turn_seq);

        -- Turn logs table
        CREATE TABLE turn_logs (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            thread_id TEXT NOT NULL REFERENCES threads(id) ON DELETE CASCADE,
            turn_seq INTEGER NOT NULL,
            input_tokens INTEGER NOT NULL,
            output_tokens INTEGER NOT NULL,
            model TEXT NOT NULL,
            latency_ms INTEGER NOT NULL,
            turn_data TEXT NOT NULL,
            created_at TEXT NOT NULL DEFAULT (datetime('now')),
            status TEXT NOT NULL DEFAULT 'completed',
            tool_calls_count INTEGER NOT NULL DEFAULT 0,
            messages_count INTEGER NOT NULL DEFAULT 0,
            UNIQUE(thread_id, turn_seq)
        );

        CREATE INDEX idx_turn_logs_thread ON turn_logs(thread_id);
        CREATE INDEX idx_turn_logs_thread_status ON turn_logs(thread_id, status);
        CREATE INDEX idx_turn_logs_thread_turn_desc ON turn_logs(thread_id, turn_seq DESC);
        "#,
    )
    .execute(&pool)
    .await
    .unwrap();

    pool
}

/// Test helper to create a test provider.
async fn create_test_provider(pool: &SqlitePool) -> ProviderId {
    let provider_id = ProviderId::new(1);

    // Create a test provider (simplified, without encryption)
    sqlx::query(
        r#"
        INSERT INTO llm_providers (id, kind, display_name, base_url, models, default_model, encrypted_api_key, api_key_nonce, is_default)
        VALUES (?, ?, ?, ?, ?, ?, X'', X'', 1)
        "#,
    )
    .bind(provider_id.inner())
    .bind("openai")
    .bind("Test Provider")
    .bind("https://api.openai.com/v1")
    .bind("[\"gpt-4\"]")
    .bind("gpt-4")
    .execute(pool)
    .await
    .unwrap();

    provider_id
}

/// Test helper to create a test agent template.
async fn create_test_template(pool: &SqlitePool, provider_id: ProviderId) -> AgentId {
    let agent_id = AgentId::new(1);

    sqlx::query(
        r#"
        INSERT INTO agents (id, display_name, description, version, provider_id, system_prompt, tool_names)
        VALUES (?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(agent_id.inner())
    .bind("Test Agent")
    .bind("A test agent")
    .bind("1.0.0")
    .bind(provider_id.inner())
    .bind("You are a helpful assistant.")
    .bind("[]")
    .execute(pool)
    .await
    .unwrap();

    agent_id
}

/// Test helper to create a test session.
async fn create_test_session(pool: &SqlitePool) -> SessionId {
    let result = sqlx::query("INSERT INTO sessions (name) VALUES (?) RETURNING id")
        .bind("Test Session")
        .fetch_one(pool)
        .await
        .unwrap();

    SessionId::new(result.get::<i64, _>("id"))
}

/// Test helper to create a test thread.
async fn create_test_thread(
    pool: &SqlitePool,
    session_id: SessionId,
    template_id: AgentId,
    provider_id: ProviderId,
) -> ThreadId {
    let thread_id = ThreadId::new();

    sqlx::query(
        r#"
        INSERT INTO threads (id, session_id, template_id, provider_id, token_count, turn_count)
        VALUES (?, ?, ?, ?, 0, 0)
        "#,
    )
    .bind(thread_id.inner().to_string())
    .bind(session_id.inner())
    .bind(template_id.inner())
    .bind(provider_id.inner())
    .execute(pool)
    .await
    .unwrap();

    thread_id
}

/// Test helper to create test messages.
async fn create_test_messages(
    pool: &SqlitePool,
    thread_id: &ThreadId,
    turn_seq: u32,
) {
    let messages = vec![
        (1, "system", "You are a helpful assistant."),
        (2, "user", "Hello, how are you?"),
        (3, "assistant", "I'm doing well, thank you!"),
    ];

    for (seq, role, content) in messages {
        sqlx::query(
            r#"
            INSERT INTO messages (thread_id, seq, turn_seq, role, content)
            VALUES (?, ?, ?, ?, ?)
            "#,
        )
        .bind(thread_id.inner().to_string())
        .bind(seq)
        .bind(turn_seq as i64)
        .bind(role)
        .bind(content)
        .execute(pool)
        .await
        .unwrap();
    }
}

/// Test helper to create test checkpoints.
async fn create_test_checkpoint(
    pool: &SqlitePool,
    thread_id: &ThreadId,
    turn_seq: u32,
    model: &str,
) {
    let turn_data = serde_json::json!({
        "messages": [],
        "tool_calls": [],
        "llm_response": null,
        "config": "{}",
        "timestamp": Utc::now().to_rfc3339()
    });

    sqlx::query(
        r#"
        INSERT INTO turn_logs (thread_id, turn_seq, input_tokens, output_tokens, model, latency_ms, turn_data, status, tool_calls_count, messages_count)
        VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
        "#,
    )
    .bind(thread_id.inner().to_string())
    .bind(turn_seq as i64)
    .bind(100)
    .bind(50)
    .bind(model)
    .bind(1000)
    .bind(turn_data.to_string())
    .bind("completed")
    .bind(0)
    .bind(3)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn test_list_checkpoints_empty() {
    let pool = create_test_pool().await;
    let tool_manager = Arc::new(ToolManager::new());
    let compactor_manager = Arc::new(CompactorManager::with_defaults());
    let template_manager = Arc::new(TemplateManager::new(pool.clone()));

    // Create a mock provider resolver
    struct MockResolver;
    #[async_trait::async_trait]
    impl argus_session::ProviderResolver for MockResolver {
        async fn resolve(&self, _id: ProviderId) -> Result<Arc<dyn argus_protocol::LlmProvider>> {
            Err(argus_protocol::ArgusError::ProviderNotFound(1))
        }
        async fn default_provider(&self) -> Result<Arc<dyn argus_protocol::LlmProvider>> {
            Err(argus_protocol::ArgusError::DefaultProviderNotConfigured)
        }
    }

    let session_manager = SessionManager::new(
        pool,
        template_manager,
        Arc::new(MockResolver),
        tool_manager,
        compactor_manager,
    );

    let provider_id = create_test_provider(&session_manager.pool()).await;
    let template_id = create_test_template(&session_manager.pool(), provider_id).await;
    let session_id = create_test_session(&session_manager.pool()).await;
    let thread_id =
        create_test_thread(&session_manager.pool(), session_id, template_id, provider_id).await;

    // Test listing checkpoints when none exist
    let checkpoints = session_manager
        .list_checkpoints(session_id, thread_id)
        .await
        .unwrap();

    assert!(checkpoints.is_empty());
}

#[tokio::test]
async fn test_list_checkpoints_with_data() {
    let pool = create_test_pool().await;
    let tool_manager = Arc::new(ToolManager::new());
    let compactor_manager = Arc::new(CompactorManager::with_defaults());
    let template_manager = Arc::new(TemplateManager::new(pool.clone()));

    struct MockResolver;
    #[async_trait::async_trait]
    impl argus_session::ProviderResolver for MockResolver {
        async fn resolve(&self, _id: ProviderId) -> Result<Arc<dyn argus_protocol::LlmProvider>> {
            Err(argus_protocol::ArgusError::ProviderNotFound(1))
        }
        async fn default_provider(&self) -> Result<Arc<dyn argus_protocol::LlmProvider>> {
            Err(argus_protocol::ArgusError::DefaultProviderNotConfigured)
        }
    }

    let session_manager = SessionManager::new(
        pool,
        template_manager,
        Arc::new(MockResolver),
        tool_manager,
        compactor_manager,
    );

    let provider_id = create_test_provider(&session_manager.pool()).await;
    let template_id = create_test_template(&session_manager.pool(), provider_id).await;
    let session_id = create_test_session(&session_manager.pool()).await;
    let thread_id =
        create_test_thread(&session_manager.pool(), session_id, template_id, provider_id).await;

    // Create test checkpoints
    create_test_checkpoint(&session_manager.pool(), &thread_id, 1, "gpt-4").await;
    create_test_checkpoint(&session_manager.pool(), &thread_id, 2, "gpt-4").await;
    create_test_checkpoint(&session_manager.pool(), &thread_id, 3, "gpt-3.5-turbo").await;

    // Test listing checkpoints
    let checkpoints = session_manager
        .list_checkpoints(session_id, thread_id)
        .await
        .unwrap();

    assert_eq!(checkpoints.len(), 3);
    assert_eq!(checkpoints[0].turn_seq, 1);
    assert_eq!(checkpoints[1].turn_seq, 2);
    assert_eq!(checkpoints[2].turn_seq, 3);
    assert_eq!(checkpoints[0].model, "gpt-4");
    assert_eq!(checkpoints[2].model, "gpt-3.5-turbo");
}

#[tokio::test]
async fn test_get_checkpoint() {
    let pool = create_test_pool().await;
    let tool_manager = Arc::new(ToolManager::new());
    let compactor_manager = Arc::new(CompactorManager::with_defaults());
    let template_manager = Arc::new(TemplateManager::new(pool.clone()));

    struct MockResolver;
    #[async_trait::async_trait]
    impl argus_session::ProviderResolver for MockResolver {
        async fn resolve(&self, _id: ProviderId) -> Result<Arc<dyn argus_protocol::LlmProvider>> {
            Err(argus_protocol::ArgusError::ProviderNotFound(1))
        }
        async fn default_provider(&self) -> Result<Arc<dyn argus_protocol::LlmProvider>> {
            Err(argus_protocol::ArgusError::DefaultProviderNotConfigured)
        }
    }

    let session_manager = SessionManager::new(
        pool,
        template_manager,
        Arc::new(MockResolver),
        tool_manager,
        compactor_manager,
    );

    let provider_id = create_test_provider(&session_manager.pool()).await;
    let template_id = create_test_template(&session_manager.pool(), provider_id).await;
    let session_id = create_test_session(&session_manager.pool()).await;
    let thread_id =
        create_test_thread(&session_manager.pool(), session_id, template_id, provider_id).await;

    // Create test checkpoint
    create_test_checkpoint(&session_manager.pool(), &thread_id, 1, "gpt-4").await;

    // Test getting checkpoint
    let checkpoint = session_manager
        .get_checkpoint(session_id, thread_id, 1)
        .await
        .unwrap();

    assert_eq!(checkpoint.turn_seq, 1);
    assert_eq!(checkpoint.model, "gpt-4");
    assert_eq!(checkpoint.token_usage.input_tokens, 100);
    assert_eq!(checkpoint.token_usage.output_tokens, 50);
    assert_eq!(checkpoint.latency_ms, 1000);
}

#[tokio::test]
async fn test_get_checkpoint_not_found() {
    let pool = create_test_pool().await;
    let tool_manager = Arc::new(ToolManager::new());
    let compactor_manager = Arc::new(CompactorManager::with_defaults());
    let template_manager = Arc::new(TemplateManager::new(pool.clone()));

    struct MockResolver;
    #[async_trait::async_trait]
    impl argus_session::ProviderResolver for MockResolver {
        async fn resolve(&self, _id: ProviderId) -> Result<Arc<dyn argus_protocol::LlmProvider>> {
            Err(argus_protocol::ArgusError::ProviderNotFound(1))
        }
        async fn default_provider(&self) -> Result<Arc<dyn argus_protocol::LlmProvider>> {
            Err(argus_protocol::ArgusError::DefaultProviderNotConfigured)
        }
    }

    let session_manager = SessionManager::new(
        pool,
        template_manager,
        Arc::new(MockResolver),
        tool_manager,
        compactor_manager,
    );

    let provider_id = create_test_provider(&session_manager.pool()).await;
    let template_id = create_test_template(&session_manager.pool(), provider_id).await;
    let session_id = create_test_session(&session_manager.pool()).await;
    let thread_id =
        create_test_thread(&session_manager.pool(), session_id, template_id, provider_id).await;

    // Test getting non-existent checkpoint
    let result = session_manager
        .get_checkpoint(session_id, thread_id, 999)
        .await;

    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        argus_protocol::ArgusError::CheckpointNotFound { .. }
    ));
}

#[tokio::test]
async fn test_compare_checkpoints() {
    let pool = create_test_pool().await;
    let tool_manager = Arc::new(ToolManager::new());
    let compactor_manager = Arc::new(CompactorManager::with_defaults());
    let template_manager = Arc::new(TemplateManager::new(pool.clone()));

    struct MockResolver;
    #[async_trait::async_trait]
    impl argus_session::ProviderResolver for MockResolver {
        async fn resolve(&self, _id: ProviderId) -> Result<Arc<dyn argus_protocol::LlmProvider>> {
            Err(argus_protocol::ArgusError::ProviderNotFound(1))
        }
        async fn default_provider(&self) -> Result<Arc<dyn argus_protocol::LlmProvider>> {
            Err(argus_protocol::ArgusError::DefaultProviderNotConfigured)
        }
    }

    let session_manager = SessionManager::new(
        pool,
        template_manager,
        Arc::new(MockResolver),
        tool_manager,
        compactor_manager,
    );

    let provider_id = create_test_provider(&session_manager.pool()).await;
    let template_id = create_test_template(&session_manager.pool(), provider_id).await;
    let session_id = create_test_session(&session_manager.pool()).await;
    let thread_id =
        create_test_thread(&session_manager.pool(), session_id, template_id, provider_id).await;

    // Create test checkpoints with different token usage
    create_test_checkpoint(&session_manager.pool(), &thread_id, 1, "gpt-4").await;
    create_test_checkpoint(&session_manager.pool(), &thread_id, 2, "gpt-4").await;

    // Update second checkpoint to have different token usage
    sqlx::query(
        r#"
        UPDATE turn_logs
        SET input_tokens = 200, output_tokens = 100
        WHERE turn_seq = 2
        "#,
    )
    .execute(&*session_manager.pool())
    .await
    .unwrap();

    // Test comparing checkpoints
    let comparison = session_manager
        .compare_checkpoints(session_id, thread_id, 1, 2)
        .await
        .unwrap();

    assert_eq!(comparison.turn_a.turn_seq, 1);
    assert_eq!(comparison.turn_b.turn_seq, 2);
    assert_eq!(comparison.token_diff.input_delta, 100);
    assert_eq!(comparison.token_diff.output_delta, 50);
    assert_eq!(comparison.token_diff.total_delta, 150);
}

#[tokio::test]
async fn test_rollback_to_turn() {
    let pool = create_test_pool().await;
    let tool_manager = Arc::new(ToolManager::new());
    let compactor_manager = Arc::new(CompactorManager::with_defaults());
    let template_manager = Arc::new(TemplateManager::new(pool.clone()));

    struct MockResolver;
    #[async_trait::async_trait]
    impl argus_session::ProviderResolver for MockResolver {
        async fn resolve(&self, _id: ProviderId) -> Result<Arc<dyn argus_protocol::LlmProvider>> {
            Err(argus_protocol::ArgusError::ProviderNotFound(1))
        }
        async fn default_provider(&self) -> Result<Arc<dyn argus_protocol::LlmProvider>> {
            Err(argus_protocol::ArgusError::DefaultProviderNotConfigured)
        }
    }

    let session_manager = SessionManager::new(
        pool,
        template_manager,
        Arc::new(MockResolver),
        tool_manager,
        compactor_manager,
    );

    let provider_id = create_test_provider(&session_manager.pool()).await;
    let template_id = create_test_template(&session_manager.pool(), provider_id).await;
    let session_id = create_test_session(&session_manager.pool()).await;
    let thread_id =
        create_test_thread(&session_manager.pool(), session_id, template_id, provider_id).await;

    // Create test messages for 3 turns
    create_test_messages(&session_manager.pool(), &thread_id, 1).await;
    create_test_messages(&session_manager.pool(), &thread_id, 2).await;
    create_test_messages(&session_manager.pool(), &thread_id, 3).await;

    // Create test checkpoints
    create_test_checkpoint(&session_manager.pool(), &thread_id, 1, "gpt-4").await;
    create_test_checkpoint(&session_manager.pool(), &thread_id, 2, "gpt-4").await;
    create_test_checkpoint(&session_manager.pool(), &thread_id, 3, "gpt-4").await;

    // Load the session to populate the thread cache
    session_manager.load(session_id).await.unwrap();

    // Verify initial state
    let initial_checkpoints = session_manager
        .list_checkpoints(session_id, thread_id)
        .await
        .unwrap();
    assert_eq!(initial_checkpoints.len(), 3);

    // Rollback to turn 2
    let state = session_manager
        .rollback_to_turn(session_id, thread_id, 2)
        .await
        .unwrap();

    // Verify rollback
    assert_eq!(state.last_turn_seq, Some(2));
    assert_eq!(state.turn_count, 2);

    // Verify only 2 checkpoints remain
    let rolled_back_checkpoints = session_manager
        .list_checkpoints(session_id, thread_id)
        .await
        .unwrap();
    assert_eq!(rolled_back_checkpoints.len(), 2);
    assert_eq!(rolled_back_checkpoints[1].turn_seq, 2);
}

#[tokio::test]
async fn test_get_history_at_turn() {
    let pool = create_test_pool().await;
    let tool_manager = Arc::new(ToolManager::new());
    let compactor_manager = Arc::new(CompactorManager::with_defaults());
    let template_manager = Arc::new(TemplateManager::new(pool.clone()));

    struct MockResolver;
    #[async_trait::async_trait]
    impl argus_session::ProviderResolver for MockResolver {
        async fn resolve(&self, _id: ProviderId) -> Result<Arc<dyn argus_protocol::LlmProvider>> {
            Err(argus_protocol::ArgusError::ProviderNotFound(1))
        }
        async fn default_provider(&self) -> Result<Arc<dyn argus_protocol::LlmProvider>> {
            Err(argus_protocol::ArgusError::DefaultProviderNotConfigured)
        }
    }

    let session_manager = SessionManager::new(
        pool,
        template_manager,
        Arc::new(MockResolver),
        tool_manager,
        compactor_manager,
    );

    let provider_id = create_test_provider(&session_manager.pool()).await;
    let template_id = create_test_template(&session_manager.pool(), provider_id).await;
    let session_id = create_test_session(&session_manager.pool()).await;
    let thread_id =
        create_test_thread(&session_manager.pool(), session_id, template_id, provider_id).await;

    // Create test messages for 2 turns
    create_test_messages(&session_manager.pool(), &thread_id, 1).await;
    create_test_messages(&session_manager.pool(), &thread_id, 2).await;

    // Test getting history at turn 1
    let history = session_manager
        .get_history_at_turn(session_id, thread_id, 1)
        .await
        .unwrap();

    assert_eq!(history.len(), 3); // 3 messages in turn 1
}

#[tokio::test]
async fn test_get_recent_messages() {
    let pool = create_test_pool().await;
    let tool_manager = Arc::new(ToolManager::new());
    let compactor_manager = Arc::new(CompactorManager::with_defaults());
    let template_manager = Arc::new(TemplateManager::new(pool.clone()));

    struct MockResolver;
    #[async_trait::async_trait]
    impl argus_session::ProviderResolver for MockResolver {
        async fn resolve(&self, _id: ProviderId) -> Result<Arc<dyn argus_protocol::LlmProvider>> {
            Err(argus_protocol::ArgusError::ProviderNotFound(1))
        }
        async fn default_provider(&self) -> Result<Arc<dyn argus_protocol::LlmProvider>> {
            Err(argus_protocol::ArgusError::DefaultProviderNotConfigured)
        }
    }

    let session_manager = SessionManager::new(
        pool,
        template_manager,
        Arc::new(MockResolver),
        tool_manager,
        compactor_manager,
    );

    let provider_id = create_test_provider(&session_manager.pool()).await;
    let template_id = create_test_template(&session_manager.pool(), provider_id).await;
    let session_id = create_test_session(&session_manager.pool()).await;
    let thread_id =
        create_test_thread(&session_manager.pool(), session_id, template_id, provider_id).await;

    // Create test messages for 2 turns (6 messages total)
    create_test_messages(&session_manager.pool(), &thread_id, 1).await;
    create_test_messages(&session_manager.pool(), &thread_id, 2).await;

    // Test getting recent 5 messages
    let recent = session_manager
        .get_recent_messages(session_id, thread_id, 5)
        .await
        .unwrap();

    assert_eq!(recent.len(), 5); // Last 5 messages
}
