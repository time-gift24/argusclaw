//! Integration tests for committed turn-log persistence.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;

use tokio::sync::{RwLock, broadcast};
use tokio::time::{Duration, sleep, timeout};

use argus_agent::trace::TraceConfig;
use argus_agent::turn_log_store::recover_thread_log_state;
use argus_agent::{
    CompactError, CompactResult, Compactor, Thread, ThreadBuilder, ThreadConfig, TurnBuilder,
    TurnConfigBuilder,
};
use argus_protocol::llm::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LlmError, LlmEventStream,
    LlmProvider, LlmStreamEvent,
};
use argus_protocol::{AgentRecord, SessionId, ThreadEvent};
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

struct SequencedDelayedMockProvider {
    delay: Duration,
    responses: Mutex<VecDeque<String>>,
    captured_user_inputs: Mutex<Vec<String>>,
}

impl SequencedDelayedMockProvider {
    fn new(delay: Duration, responses: Vec<&str>) -> Self {
        Self {
            delay,
            responses: Mutex::new(
                responses
                    .into_iter()
                    .map(ToString::to_string)
                    .collect::<VecDeque<_>>(),
            ),
            captured_user_inputs: Mutex::new(Vec::new()),
        }
    }

    fn captured_user_inputs(&self) -> Vec<String> {
        self.captured_user_inputs.lock().unwrap().clone()
    }

    fn next_response(&self) -> String {
        self.responses
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| "default response".to_string())
    }

    fn capture_request(&self, request: &CompletionRequest) {
        let last_user_input = request
            .messages
            .iter()
            .rev()
            .find(|message| message.role == argus_protocol::llm::Role::User)
            .map(|message| message.content.clone())
            .unwrap_or_default();
        self.captured_user_inputs
            .lock()
            .unwrap()
            .push(last_user_input);
    }
}

#[async_trait]
impl LlmProvider for SequencedDelayedMockProvider {
    fn model_name(&self) -> &str {
        "sequenced-delayed"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        self.capture_request(&request);
        sleep(self.delay).await;

        Ok(CompletionResponse {
            content: Some(self.next_response()),
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
        request: CompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        self.capture_request(&request);
        let delay = self.delay;
        let response = self.next_response();
        let stream = futures_util::stream::once(async move {
            sleep(delay).await;
            Ok(LlmStreamEvent::ContentDelta { delta: response })
        });
        Ok(Box::pin(stream))
    }
}

struct NoopCompactor;

#[async_trait]
impl Compactor for NoopCompactor {
    async fn compact(
        &self,
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

async fn wait_for_provider_inputs(
    provider: &Arc<SequencedDelayedMockProvider>,
    expected_len: usize,
) {
    timeout(Duration::from_secs(5), async {
        loop {
            if provider.captured_user_inputs().len() >= expected_len {
                break;
            }
            sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("provider should capture user inputs");
}

async fn collect_terminal_events_until_final_idle(
    mut rx: broadcast::Receiver<ThreadEvent>,
    expected_turns: usize,
) -> Vec<ThreadEvent> {
    let mut events = Vec::new();
    let mut settled_count = 0usize;

    timeout(Duration::from_secs(5), async {
        loop {
            match rx.recv().await {
                Ok(event @ ThreadEvent::TurnCompleted { .. })
                | Ok(event @ ThreadEvent::TurnFailed { .. })
                | Ok(event @ ThreadEvent::TurnSettled { .. })
                | Ok(event @ ThreadEvent::Idle { .. }) => {
                    if matches!(event, ThreadEvent::TurnSettled { .. }) {
                        settled_count += 1;
                    }
                    let is_final_idle = matches!(event, ThreadEvent::Idle { .. })
                        && settled_count >= expected_turns;
                    events.push(event);
                    if is_final_idle {
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => {}
            }
        }
    })
    .await
    .expect("thread should emit terminal events");

    events
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
            .agent_record(Arc::new(AgentRecord {
                system_prompt: "You are a test assistant.".to_string(),
                ..AgentRecord::default()
            }))
            .session_id(session_id)
            .config(thread_config)
            .build()
            .unwrap(),
    ));
    let thread_id = { thread.read().await.id() };
    let rx = { thread.read().await.subscribe() };

    Thread::spawn_reactor(Arc::clone(&thread));

    {
        let guard = thread.read().await;
        guard
            .send_user_message("Hello".to_string(), None)
            .expect("message should queue");
    }

    wait_for_turn_settled_event(rx).await;

    let persisted_base_dir = temp_dir
        .path()
        .join(session_id.to_string())
        .join(thread_id.to_string());

    timeout(Duration::from_secs(5), async {
        loop {
            let recovered = recover_thread_log_state(&persisted_base_dir)
                .await
                .expect("recovery should work");
            if recovered
                .turns
                .iter()
                .any(|t| matches!(t.kind, argus_agent::history::TurnRecordKind::UserTurn))
            {
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("committed turn logs should be persisted");

    assert_eq!(thread.read().await.turn_count(), 1);

    let recovered = recover_thread_log_state(&persisted_base_dir)
        .await
        .expect("recovery should work");
    let user_turn = recovered
        .turns
        .iter()
        .find(|t| matches!(t.kind, argus_agent::history::TurnRecordKind::UserTurn))
        .expect("should have user turn");
    assert_eq!(user_turn.messages.len(), 2);
    assert_eq!(user_turn.messages[0].content, "Hello");
    assert_eq!(user_turn.messages[1].content, "Hello, world!");
    assert_eq!(user_turn.turn_number, Some(1));
    assert!(matches!(
        user_turn.state,
        argus_agent::history::TurnState::Completed
    ));
    assert!(user_turn.token_usage.is_some());
}

#[tokio::test]
async fn test_thread_runtime_queues_follow_up_turn_without_emitting_idle_between_settlements() {
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
    let provider = Arc::new(SequencedDelayedMockProvider::new(
        Duration::from_millis(120),
        vec!["first reply", "second reply"],
    ));
    let thread = Arc::new(RwLock::new(
        ThreadBuilder::new()
            .provider(provider.clone())
            .compactor(Arc::new(NoopCompactor))
            .agent_record(Arc::new(AgentRecord {
                system_prompt: "You are a test assistant.".to_string(),
                ..AgentRecord::default()
            }))
            .session_id(session_id)
            .config(thread_config)
            .build()
            .unwrap(),
    ));
    let thread_id = { thread.read().await.id() };
    let rx = { thread.read().await.subscribe() };

    Thread::spawn_reactor(Arc::clone(&thread));

    {
        let guard = thread.read().await;
        guard
            .send_user_message("first".to_string(), None)
            .expect("first message should queue");
    }

    wait_for_provider_inputs(&provider, 1).await;

    {
        let guard = thread.read().await;
        guard
            .send_user_message("second".to_string(), None)
            .expect("second message should queue");
    }

    let terminal_events = collect_terminal_events_until_final_idle(rx, 2).await;
    let filtered_events = terminal_events
        .into_iter()
        .filter(|event| {
            matches!(
                event,
                ThreadEvent::TurnCompleted { .. }
                    | ThreadEvent::TurnFailed { .. }
                    | ThreadEvent::TurnSettled { .. }
                    | ThreadEvent::Idle { .. }
            )
        })
        .collect::<Vec<_>>();

    assert_eq!(
        filtered_events.len(),
        5,
        "two queued turns should emit completed/settled pairs followed by a single final idle",
    );
    assert!(matches!(
        filtered_events[0],
        ThreadEvent::TurnCompleted { turn_number: 1, .. }
    ));
    assert!(matches!(
        filtered_events[1],
        ThreadEvent::TurnSettled { turn_number: 1, .. }
    ));
    assert!(matches!(
        filtered_events[2],
        ThreadEvent::TurnCompleted { turn_number: 2, .. }
    ));
    assert!(matches!(
        filtered_events[3],
        ThreadEvent::TurnSettled { turn_number: 2, .. }
    ));
    assert!(matches!(filtered_events[4], ThreadEvent::Idle { .. }));

    assert_eq!(
        provider.captured_user_inputs(),
        vec!["first".to_string(), "second".to_string()],
    );

    let persisted_base_dir = temp_dir
        .path()
        .join(session_id.to_string())
        .join(thread_id.to_string());

    timeout(Duration::from_secs(5), async {
        loop {
            let recovered = recover_thread_log_state(&persisted_base_dir)
                .await
                .expect("recovery should work");
            let user_turn_count = recovered
                .turns
                .iter()
                .filter(|t| matches!(t.kind, argus_agent::history::TurnRecordKind::UserTurn))
                .count();
            if user_turn_count == 2 {
                break;
            }
            sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .expect("committed turn logs should be persisted");

    let recovered = recover_thread_log_state(&persisted_base_dir)
        .await
        .expect("recovery should work");
    for expected_turn_number in [1_u32, 2_u32] {
        let user_turn = recovered
            .turns
            .iter()
            .find(|t| {
                matches!(t.kind, argus_agent::history::TurnRecordKind::UserTurn)
                    && t.turn_number == Some(expected_turn_number)
            })
            .expect("should have user turn");
        assert_eq!(user_turn.turn_number, Some(expected_turn_number));
        assert!(matches!(
            user_turn.state,
            argus_agent::history::TurnState::Completed
        ));
    }
}
