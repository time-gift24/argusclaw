//! Integration tests for committed turn-log persistence.

use std::sync::Arc;

use tokio::sync::{RwLock, broadcast};
use tokio::time::{Duration, sleep, timeout};

use argus_agent::trace::TraceConfig;
use argus_agent::turn_log_store::{
    read_turn_meta, read_turn_messages, turn_messages_path, turn_meta_path, turns_dir,
};
use argus_agent::{
    CompactError, CompactResult, Compactor, Thread, ThreadBuilder, ThreadConfig, TurnBuilder,
    TurnConfigBuilder,
};
use argus_protocol::{AgentRecord, SessionId, ThreadEvent};
use argus_protocol::llm::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LlmError, LlmEventStream,
    LlmProvider, LlmStreamEvent,
};
use async_trait::async_trait;
use rust_decimal::Decimal;

/// Mock provider that returns a simple response
struct SimpleMockProvider {
    response: String,
}

impl SimpleMockProvider {
    fn new(response: &str) -> Self {
        Self {
            response: response.to_string(),
        }
    }
}

#[async_trait]
impl LlmProvider for SimpleMockProvider {
    fn model_name(&self) -> &str {
        "mock"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        Ok(CompletionResponse {
            content: Some(self.response.clone()),
            reasoning_content: None,
            tool_calls: vec![],
            input_tokens: 10,
            output_tokens: 5,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }

    async fn stream_complete(
        &self,
        _request: CompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        let response = self.response.clone();
        let stream = futures_util::stream::once(async move {
            Ok(LlmStreamEvent::ContentDelta { delta: response })
        });
        Ok(Box::pin(stream))
    }
}

struct PartialFailureMockProvider;

#[async_trait]
impl LlmProvider for PartialFailureMockProvider {
    fn model_name(&self) -> &str {
        "partial-failure"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        Err(LlmError::UnsupportedCapability {
            provider: "partial-failure".to_string(),
            capability: "complete".to_string(),
        })
    }

    async fn stream_complete(
        &self,
        _request: CompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        let stream = futures_util::stream::iter(vec![
            Ok(LlmStreamEvent::ContentDelta {
                delta: "partial answer".to_string(),
            }),
            Err(LlmError::RequestFailed {
                provider: "partial-failure".to_string(),
                reason: "stream timeout".to_string(),
            }),
        ]);
        Ok(Box::pin(stream))
    }
}

struct NoopCompactor;

#[async_trait]
impl Compactor for NoopCompactor {
    async fn compact(
        &self,
        _provider: &dyn LlmProvider,
        _messages: &[ChatMessage],
        _token_count: u32,
    ) -> Result<Option<CompactResult>, CompactError> {
        Ok(None)
    }

    fn name(&self) -> &'static str {
        "noop"
    }
}

async fn wait_for_turn_settled_event(mut rx: broadcast::Receiver<ThreadEvent>) {
    timeout(Duration::from_secs(5), async {
        loop {
            match rx.recv().await {
                Ok(ThreadEvent::TurnSettled { .. }) => break,
                Ok(_) => {}
                Err(_) => {}
            }
        }
    })
    .await
    .expect("thread should emit turn_settled");
}

#[tokio::test]
async fn test_turn_trace_disabled_by_default() {
    // Default config should have tracing disabled
    let config = TraceConfig::default();
    assert!(!config.enabled, "Trace should be disabled by default");
}

#[tokio::test]
async fn test_turn_execute_does_not_write_legacy_event_trace_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let trace_config = TraceConfig::new(true, temp_dir.path().to_path_buf());

    let provider = Arc::new(SimpleMockProvider::new("Hello, world!"));
    let (stream_tx, _) = broadcast::channel(256);
    let (thread_event_tx, _) = broadcast::channel(256);

    let turn = TurnBuilder::default()
        .turn_number(1)
        .thread_id("test-thread-no-legacy-trace".to_string())
        .messages(vec![ChatMessage::user("Hello")])
        .provider(provider)
        .agent_record(Arc::new(AgentRecord::default()))
        .tools(vec![])
        .hooks(vec![])
        .config(
            TurnConfigBuilder::default()
                .trace_config(trace_config)
                .build()
                .unwrap(),
        )
        .stream_tx(stream_tx)
        .thread_event_tx(thread_event_tx)
        .build()
        .unwrap();

    let _output = turn.execute().await.unwrap();

    let legacy_trace_path = temp_dir
        .path()
        .join("test-thread-no-legacy-trace")
        .join("turns")
        .join("1.jsonl");
    assert!(
        !legacy_trace_path.exists(),
        "legacy event trace should not be persisted at {:?}",
        legacy_trace_path
    );
}

#[tokio::test]
async fn test_failed_turn_does_not_write_legacy_partial_trace_file() {
    let temp_dir = tempfile::tempdir().unwrap();
    let trace_config = TraceConfig::new(true, temp_dir.path().to_path_buf());

    let provider = Arc::new(PartialFailureMockProvider);
    let (stream_tx, _) = broadcast::channel(256);
    let (thread_event_tx, _) = broadcast::channel(256);

    let turn = TurnBuilder::default()
        .turn_number(1)
        .thread_id("test-thread-no-partial-trace".to_string())
        .messages(vec![ChatMessage::user("Hello")])
        .provider(provider)
        .agent_record(Arc::new(AgentRecord::default()))
        .tools(vec![])
        .hooks(vec![])
        .config(
            TurnConfigBuilder::default()
                .trace_config(trace_config)
                .build()
                .unwrap(),
        )
        .stream_tx(stream_tx)
        .thread_event_tx(thread_event_tx)
        .build()
        .unwrap();

    let error = turn
        .execute()
        .await
        .expect_err("stream failure should fail the turn");
    assert!(matches!(error, argus_agent::TurnError::LlmFailed(_)));

    let legacy_trace_path = temp_dir
        .path()
        .join("test-thread-no-partial-trace")
        .join("turns")
        .join("1.jsonl");
    assert!(
        !legacy_trace_path.exists(),
        "failed turn should not leave a legacy partial trace at {:?}",
        legacy_trace_path
    );
}

#[tokio::test]
async fn test_thread_runtime_persists_committed_turn_messages_and_meta() {
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = SessionId::new();
    let trace_config =
        TraceConfig::new(true, temp_dir.path().to_path_buf()).with_session_id(session_id);
    let thread_config = ThreadConfig {
        turn_config: TurnConfigBuilder::default()
            .trace_config(trace_config)
            .build()
            .unwrap(),
    };
    let thread = Arc::new(RwLock::new(
        ThreadBuilder::new()
            .provider(Arc::new(SimpleMockProvider::new("Hello, world!")))
            .compactor(Arc::new(NoopCompactor))
            .agent_record(Arc::new(AgentRecord::default()))
            .session_id(session_id)
            .config(thread_config)
            .build()
            .unwrap(),
    ));
    let thread_id = { thread.read().await.id() };
    let rx = { thread.read().await.subscribe() };

    Thread::spawn_runtime_actor(Arc::clone(&thread));

    {
        let guard = thread.read().await;
        guard
            .send_user_message("Hello".to_string(), None)
            .expect("message should queue");
    }

    wait_for_turn_settled_event(rx).await;

    let persisted_turns_dir = turns_dir(
        &temp_dir
            .path()
            .join(session_id.to_string())
            .join(thread_id.to_string()),
    );
    let messages_path = turn_messages_path(&persisted_turns_dir, 1);
    let meta_path = turn_meta_path(&persisted_turns_dir, 1);

    timeout(Duration::from_secs(5), async {
        loop {
            let messages_exists = tokio::fs::try_exists(&messages_path).await.unwrap_or(false);
            let meta_exists = tokio::fs::try_exists(&meta_path).await.unwrap_or(false);
            if messages_exists && meta_exists {
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("committed turn logs should be persisted");

    assert_eq!(thread.read().await.turn_count(), 1);

    let messages = read_turn_messages(&messages_path)
        .await
        .expect("messages should read");
    let meta = read_turn_meta(&meta_path).await.expect("meta should read");

    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].content, "Hello");
    assert_eq!(messages[1].content, "Hello, world!");
    assert_eq!(meta.turn_number, 1);
    assert!(matches!(meta.state, argus_agent::history::TurnState::Completed));
    assert!(meta.token_usage.is_some());
}
