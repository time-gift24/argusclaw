//! Integration tests for committed turn-log persistence.

use std::collections::VecDeque;
use std::sync::Arc;
use std::sync::Mutex;

use tokio::sync::{RwLock, broadcast};
use tokio::time::{Duration, sleep, timeout};

use argus_agent::thread_trace_store::chat_thread_base_dir;
use argus_agent::trace::TraceConfig;
use argus_agent::turn_log_store::recover_thread_log_state;
use argus_agent::{
    CompactError, Thread, ThreadBuilder, ThreadCompactResult, ThreadCompactor, ThreadConfig,
    TurnBuilder, TurnConfigBuilder,
};
use argus_protocol::llm::{
    ChatMessage, CompletionRequest, CompletionResponse, FinishReason, LlmError, LlmEventStream,
    LlmProvider, LlmStreamEvent,
};
use argus_protocol::{AgentRecord, SessionId, ThreadEvent};
use async_trait::async_trait;
use rust_decimal::Decimal;

async fn enqueue_thread_message(thread: &Arc<RwLock<Thread>>, message: String) {
    let mailbox = {
        let guard = thread.read().await;
        guard.mailbox()
    };
    mailbox.lock().await.enqueue_user_message(message, None);
    let guard = thread.read().await;
    let _ = guard
        .control_tx()
        .send(argus_protocol::ThreadControlEvent::MailboxUpdated);
}

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

struct RuntimeCompactionProvider {
    delay: Duration,
    complete_responses: Mutex<VecDeque<CompletionResponse>>,
    stream_results: Mutex<VecDeque<Result<String, LlmError>>>,
    context_window: u32,
}

impl RuntimeCompactionProvider {
    fn new(
        delay: Duration,
        complete_responses: Vec<CompletionResponse>,
        stream_results: Vec<Result<&str, LlmError>>,
        context_window: u32,
    ) -> Self {
        Self {
            delay,
            complete_responses: Mutex::new(complete_responses.into_iter().collect()),
            stream_results: Mutex::new(
                stream_results
                    .into_iter()
                    .map(|item| item.map(str::to_string))
                    .collect(),
            ),
            context_window,
        }
    }
}

#[async_trait]
impl LlmProvider for RuntimeCompactionProvider {
    fn model_name(&self) -> &str {
        "runtime-compaction"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        self.complete_responses
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| LlmError::RequestFailed {
                provider: "runtime-compaction".to_string(),
                reason: "missing complete response".to_string(),
            })
    }

    async fn stream_complete(
        &self,
        _request: CompletionRequest,
    ) -> Result<LlmEventStream, LlmError> {
        let delay = self.delay;
        let next = self
            .stream_results
            .lock()
            .unwrap()
            .pop_front()
            .ok_or_else(|| LlmError::RequestFailed {
                provider: "runtime-compaction".to_string(),
                reason: "missing stream response".to_string(),
            })?;
        match next {
            Ok(response) => {
                let stream = futures_util::stream::once(async move {
                    sleep(delay).await;
                    Ok(LlmStreamEvent::ContentDelta { delta: response })
                });
                Ok(Box::pin(stream))
            }
            Err(error) => Ok(Box::pin(futures_util::stream::once(
                async move { Err(error) },
            ))),
        }
    }

    fn context_window(&self) -> u32 {
        self.context_window
    }
}

struct NoopCompactor;

#[async_trait]
impl ThreadCompactor for NoopCompactor {
    async fn compact(
        &self,
        _messages: &[ChatMessage],
        _token_count: u32,
    ) -> Result<Option<ThreadCompactResult>, CompactError> {
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

async fn request_thread_stop(thread: &Arc<RwLock<Thread>>) {
    let mailbox = {
        let guard = thread.read().await;
        guard.mailbox()
    };
    mailbox.lock().await.interrupt_stop();
    let guard = thread.read().await;
    let _ = guard
        .control_tx()
        .send(argus_protocol::ThreadControlEvent::MailboxUpdated);
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
    let thread_id = argus_protocol::ThreadId::new();
    let trace_config = TraceConfig::new(
        true,
        chat_thread_base_dir(temp_dir.path(), session_id, thread_id),
    );
    let thread_config = ThreadConfig {
        turn_config: TurnConfigBuilder::default()
            .trace_config(trace_config)
            .build()
            .unwrap(),
    };
    let thread = Arc::new(RwLock::new(
        ThreadBuilder::new()
            .id(thread_id)
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
    let rx = { thread.read().await.subscribe() };

    Thread::spawn_reactor(Arc::clone(&thread));

    enqueue_thread_message(&thread, "Hello".to_string()).await;

    wait_for_turn_settled_event(rx).await;

    let persisted_base_dir = chat_thread_base_dir(temp_dir.path(), session_id, thread_id);

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
    assert_eq!(user_turn.turn_number, 1);
    assert_eq!(user_turn.token_usage.total_tokens, 0);
}

#[tokio::test]
async fn test_thread_runtime_queues_follow_up_turn_without_emitting_idle_between_settlements() {
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = SessionId::new();
    let thread_id = argus_protocol::ThreadId::new();
    let trace_config = TraceConfig::new(
        true,
        chat_thread_base_dir(temp_dir.path(), session_id, thread_id),
    );
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
            .id(thread_id)
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
    let rx = { thread.read().await.subscribe() };

    Thread::spawn_reactor(Arc::clone(&thread));

    enqueue_thread_message(&thread, "first".to_string()).await;

    wait_for_provider_inputs(&provider, 1).await;

    enqueue_thread_message(&thread, "second".to_string()).await;

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

    let persisted_base_dir = chat_thread_base_dir(temp_dir.path(), session_id, thread_id);

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
                    && t.turn_number == expected_turn_number
            })
            .expect("should have user turn");
        assert_eq!(user_turn.turn_number, expected_turn_number);
    }
}

#[tokio::test]
async fn successful_turn_compaction_persists_checkpoint_before_user_turn() {
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = SessionId::new();
    let thread_id = argus_protocol::ThreadId::new();
    let trace_config = TraceConfig::new(
        true,
        chat_thread_base_dir(temp_dir.path(), session_id, thread_id),
    );
    let thread_config = ThreadConfig {
        turn_config: TurnConfigBuilder::default()
            .trace_config(trace_config)
            .build()
            .unwrap(),
    };
    let provider = Arc::new(RuntimeCompactionProvider::new(
        Duration::from_millis(10),
        vec![CompletionResponse {
            content: Some("user-facing summary".to_string()),
            reasoning_content: None,
            tool_calls: Vec::new(),
            input_tokens: 4,
            output_tokens: 1,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        }],
        vec![Ok("final reply")],
        4,
    ));
    let thread = Arc::new(RwLock::new(
        ThreadBuilder::new()
            .id(thread_id)
            .provider(provider)
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
    let rx = { thread.read().await.subscribe() };

    Thread::spawn_reactor(Arc::clone(&thread));
    enqueue_thread_message(
        &thread,
        "this message is long enough to trigger turn compaction".to_string(),
    )
    .await;
    wait_for_turn_settled_event(rx).await;

    let recovered = recover_thread_log_state(&chat_thread_base_dir(
        temp_dir.path(),
        session_id,
        thread_id,
    ))
    .await
    .expect("recovery should work");
    assert_eq!(recovered.turns.len(), 2);
    assert!(matches!(
        recovered.turns[0].kind,
        argus_agent::history::TurnRecordKind::Checkpoint
    ));
    assert!(matches!(
        recovered.turns[1].kind,
        argus_agent::history::TurnRecordKind::UserTurn
    ));
    assert_eq!(
        recovered.turns[0]
            .messages
            .last()
            .expect("checkpoint summary")
            .content,
        "user-facing summary"
    );
    let user_turn_contents: Vec<_> = recovered.turns[1]
        .messages
        .iter()
        .map(|message| message.content.as_str())
        .collect();
    assert_eq!(user_turn_contents, vec!["final reply"]);
}

#[tokio::test]
async fn failed_turn_after_compaction_persists_no_turn_level_checkpoint() {
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = SessionId::new();
    let thread_id = argus_protocol::ThreadId::new();
    let trace_config = TraceConfig::new(
        true,
        chat_thread_base_dir(temp_dir.path(), session_id, thread_id),
    );
    let thread_config = ThreadConfig {
        turn_config: TurnConfigBuilder::default()
            .trace_config(trace_config)
            .build()
            .unwrap(),
    };
    let provider = Arc::new(RuntimeCompactionProvider::new(
        Duration::from_millis(10),
        vec![CompletionResponse {
            content: Some("user-facing summary".to_string()),
            reasoning_content: None,
            tool_calls: Vec::new(),
            input_tokens: 4,
            output_tokens: 1,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        }],
        vec![Err(LlmError::RequestFailed {
            provider: "runtime-compaction".to_string(),
            reason: "stream failed".to_string(),
        })],
        4,
    ));
    let thread = Arc::new(RwLock::new(
        ThreadBuilder::new()
            .id(thread_id)
            .provider(provider)
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
    let rx = { thread.read().await.subscribe() };

    Thread::spawn_reactor(Arc::clone(&thread));
    enqueue_thread_message(
        &thread,
        "this message is long enough to trigger turn compaction".to_string(),
    )
    .await;
    let terminal_events = collect_terminal_events_until_final_idle(rx, 1).await;
    assert!(
        terminal_events
            .iter()
            .any(|event| matches!(event, ThreadEvent::TurnFailed { .. }))
    );

    let recovered = recover_thread_log_state(&chat_thread_base_dir(
        temp_dir.path(),
        session_id,
        thread_id,
    ))
    .await
    .expect("recovery should work");
    assert!(recovered.turns.is_empty());
}

#[tokio::test]
async fn cancelled_turn_after_compaction_persists_no_turn_level_checkpoint() {
    let temp_dir = tempfile::tempdir().unwrap();
    let session_id = SessionId::new();
    let thread_id = argus_protocol::ThreadId::new();
    let trace_config = TraceConfig::new(
        true,
        chat_thread_base_dir(temp_dir.path(), session_id, thread_id),
    );
    let thread_config = ThreadConfig {
        turn_config: TurnConfigBuilder::default()
            .trace_config(trace_config)
            .build()
            .unwrap(),
    };
    let provider = Arc::new(RuntimeCompactionProvider::new(
        Duration::from_secs(1),
        vec![CompletionResponse {
            content: Some("user-facing summary".to_string()),
            reasoning_content: None,
            tool_calls: Vec::new(),
            input_tokens: 4,
            output_tokens: 1,
            finish_reason: FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        }],
        vec![Ok("slow final reply")],
        4,
    ));
    let thread = Arc::new(RwLock::new(
        ThreadBuilder::new()
            .id(thread_id)
            .provider(provider)
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
    let rx = { thread.read().await.subscribe() };

    Thread::spawn_reactor(Arc::clone(&thread));
    enqueue_thread_message(
        &thread,
        "this message is long enough to trigger turn compaction".to_string(),
    )
    .await;
    sleep(Duration::from_millis(50)).await;
    request_thread_stop(&thread).await;
    let terminal_events = collect_terminal_events_until_final_idle(rx, 1).await;
    assert!(
        terminal_events
            .iter()
            .all(|event| !matches!(event, ThreadEvent::TurnCompleted { .. })),
        "cancelled turn should not report completion"
    );

    let recovered = recover_thread_log_state(&chat_thread_base_dir(
        temp_dir.path(),
        session_id,
        thread_id,
    ))
    .await
    .expect("recovery should work");
    assert!(recovered.turns.is_empty());
}
