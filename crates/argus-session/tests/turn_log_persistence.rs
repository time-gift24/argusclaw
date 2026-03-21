use std::sync::Arc;

use argus_protocol::llm::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LlmError, LlmProvider, Role,
    ToolCompletionRequest, ToolCompletionResponse,
};
use argus_protocol::{AgentId, AgentRecord, ArgusError, ProviderId, Result, SessionId, ThreadId};
use argus_repository::{connect_path, migrate};
use argus_session::{ProviderResolver, SessionManager};
use argus_template::TemplateManager;
use argus_thread::CompactorManager;
use argus_tool::ToolManager;
use async_trait::async_trait;
use rust_decimal::Decimal;
use sqlx::{Row, SqlitePool};
use tempfile::TempDir;

struct MockProvider {
    call_count: std::sync::Mutex<u32>,
}

impl MockProvider {
    fn new() -> Self {
        Self {
            call_count: std::sync::Mutex::new(0),
        }
    }
}

#[async_trait]
impl LlmProvider for MockProvider {
    fn model_name(&self) -> &str {
        "mock-model"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(
        &self,
        _request: CompletionRequest,
    ) -> std::result::Result<CompletionResponse, LlmError> {
        Ok(CompletionResponse {
            content: "unused".to_string(),
            reasoning_content: None,
            input_tokens: 0,
            output_tokens: 0,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn complete_with_tools(
        &self,
        _request: ToolCompletionRequest,
    ) -> std::result::Result<ToolCompletionResponse, LlmError> {
        let mut call_count = self.call_count.lock().expect("lock should succeed");
        *call_count += 1;
        let reply_index = *call_count;

        Ok(ToolCompletionResponse {
            content: Some(format!("assistant-{reply_index}")),
            reasoning_content: None,
            tool_calls: Vec::new(),
            input_tokens: 10 + reply_index,
            output_tokens: 5 + reply_index,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }
}

struct StaticProviderResolver {
    provider: Arc<dyn LlmProvider>,
}

impl StaticProviderResolver {
    fn new(provider: Arc<dyn LlmProvider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl ProviderResolver for StaticProviderResolver {
    async fn resolve(&self, _id: ProviderId) -> Result<Arc<dyn LlmProvider>> {
        Ok(self.provider.clone())
    }

    async fn default_provider(&self) -> Result<Arc<dyn LlmProvider>> {
        Ok(self.provider.clone())
    }
}

struct TestContext {
    _temp_dir: TempDir,
    pool: SqlitePool,
    manager: SessionManager,
    session_id: SessionId,
    thread_id: ThreadId,
}

async fn setup_context() -> TestContext {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let db_path = temp_dir.path().join("argus-session-test.sqlite");
    let pool = connect_path(&db_path)
        .await
        .expect("sqlite should connect successfully");
    migrate(&pool).await.expect("migrations should succeed");

    let provider_id_value: i64 =
        sqlx::query_scalar("SELECT id FROM llm_providers ORDER BY id LIMIT 1")
            .fetch_one(&pool)
            .await
            .expect("default provider should exist after migration");
    let provider_id = ProviderId::new(provider_id_value);

    let template_manager = Arc::new(TemplateManager::new(pool.clone()));
    let template_id = template_manager
        .upsert(AgentRecord {
            id: AgentId::new(0),
            display_name: "Session Test Agent".to_string(),
            description: "integration test agent".to_string(),
            version: "1.0.0".to_string(),
            provider_id: Some(provider_id),
            system_prompt: "You are a test assistant.".to_string(),
            tool_names: Vec::new(),
            max_tokens: None,
            temperature: None,
            thinking_config: None,
        })
        .await
        .expect("template should upsert");

    let resolver = Arc::new(StaticProviderResolver::new(Arc::new(MockProvider::new())));
    let manager = SessionManager::new(
        pool.clone(),
        template_manager,
        resolver,
        Arc::new(ToolManager::new()),
        Arc::new(CompactorManager::with_defaults()),
    );

    let session_id = manager
        .create("test-session".to_string())
        .await
        .expect("session should create");
    let thread_id = manager
        .create_thread(session_id, template_id, Some(provider_id))
        .await
        .expect("thread should create");

    TestContext {
        _temp_dir: temp_dir,
        pool,
        manager,
        session_id,
        thread_id,
    }
}

#[tokio::test]
async fn send_message_persists_turn_log_and_thread_stats() {
    let ctx = setup_context().await;

    ctx.manager
        .send_message(ctx.session_id, &ctx.thread_id, "hello world".to_string())
        .await
        .expect("send_message should succeed");

    let turn_log_row = sqlx::query(
        "SELECT turn_seq, input_tokens, output_tokens, model, turn_data FROM turn_logs WHERE thread_id = ?",
    )
    .bind(ctx.thread_id.to_string())
    .fetch_one(&ctx.pool)
    .await
    .expect("turn log should exist");

    assert_eq!(turn_log_row.get::<i64, _>("turn_seq"), 1);
    assert_eq!(turn_log_row.get::<String, _>("model"), "mock-model");
    assert!(turn_log_row.get::<i64, _>("input_tokens") > 0);
    assert!(turn_log_row.get::<i64, _>("output_tokens") > 0);

    let turn_data: String = turn_log_row.get("turn_data");
    let messages: Vec<ChatMessage> =
        serde_json::from_str(&turn_data).expect("turn_data should deserialize");
    assert!(messages
        .iter()
        .any(|m| m.role == Role::User && m.content == "hello world"));
    assert!(messages.iter().any(|m| m.role == Role::Assistant));

    let thread_row = sqlx::query("SELECT turn_count, token_count FROM threads WHERE id = ?")
        .bind(ctx.thread_id.to_string())
        .fetch_one(&ctx.pool)
        .await
        .expect("thread row should exist");
    assert_eq!(thread_row.get::<i64, _>("turn_count"), 1);
    assert!(thread_row.get::<i64, _>("token_count") > 0);
}

#[tokio::test]
async fn send_message_increments_turn_sequence_for_multiple_turns() {
    let ctx = setup_context().await;

    ctx.manager
        .send_message(ctx.session_id, &ctx.thread_id, "first".to_string())
        .await
        .expect("first send should succeed");
    ctx.manager
        .send_message(ctx.session_id, &ctx.thread_id, "second".to_string())
        .await
        .expect("second send should succeed");

    let rows = sqlx::query("SELECT turn_seq FROM turn_logs WHERE thread_id = ? ORDER BY turn_seq")
        .bind(ctx.thread_id.to_string())
        .fetch_all(&ctx.pool)
        .await
        .expect("turn logs should list");

    let turn_seq_values: Vec<i64> = rows.into_iter().map(|row| row.get("turn_seq")).collect();
    assert_eq!(turn_seq_values, vec![1, 2]);
}

#[tokio::test]
async fn reload_session_keeps_turn_sequence_continuous() {
    let ctx = setup_context().await;

    ctx.manager
        .send_message(ctx.session_id, &ctx.thread_id, "before reload".to_string())
        .await
        .expect("send before reload should succeed");

    ctx.manager
        .unload(ctx.session_id)
        .await
        .expect("unload should succeed");
    ctx.manager
        .load(ctx.session_id)
        .await
        .expect("load should succeed");

    ctx.manager
        .send_message(ctx.session_id, &ctx.thread_id, "after reload".to_string())
        .await
        .expect("send after reload should succeed");

    let rows = sqlx::query("SELECT turn_seq FROM turn_logs WHERE thread_id = ? ORDER BY turn_seq")
        .bind(ctx.thread_id.to_string())
        .fetch_all(&ctx.pool)
        .await
        .expect("turn logs should list");

    let turn_seq_values: Vec<i64> = rows.into_iter().map(|row| row.get("turn_seq")).collect();
    assert_eq!(turn_seq_values, vec![1, 2]);
}

#[tokio::test]
async fn send_message_returns_error_when_turn_log_insert_fails() {
    let ctx = setup_context().await;

    sqlx::query(
        r#"
        INSERT INTO turn_logs (thread_id, turn_seq, input_tokens, output_tokens, model, latency_ms, turn_data, created_at)
        VALUES (?, 1, 1, 1, 'mock-model', 1, '[]', datetime('now'))
        "#,
    )
    .bind(ctx.thread_id.to_string())
    .execute(&ctx.pool)
    .await
    .expect("seed turn log should succeed");

    let error = ctx
        .manager
        .send_message(ctx.session_id, &ctx.thread_id, "conflict".to_string())
        .await
        .expect_err("send_message should fail when log insert conflicts");

    assert!(matches!(error, ArgusError::TurnLogError { .. }));
}
