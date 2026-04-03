use std::collections::VecDeque;
use std::sync::Mutex;
use std::time::Duration;

use super::*;
use crate::compact::CompactResult;
use crate::config::{ThreadConfig, TurnConfigBuilder};
use crate::error::CompactError;
use crate::history::TurnRecord;
use crate::thread_handle::ThreadHandle;
use crate::trace::TraceConfig;
use crate::turn_log_store::{
    persist_turn_log_snapshot, read_turn_messages, read_turn_meta, recover_thread_log_state,
    turn_messages_path, turn_meta_path, turns_dir,
};
use argus_protocol::llm::{CompletionRequest, CompletionResponse, LlmError};
use argus_protocol::{AgentId, AgentType, ProviderId, ThreadCommand, ThreadRuntimeState};
use async_trait::async_trait;
use rust_decimal::Decimal;
use serde_json::json;
use tokio::sync::{Notify, RwLock, oneshot};
use tokio::time::{sleep, timeout};

struct DummyProvider;

#[async_trait]
impl LlmProvider for DummyProvider {
    fn model_name(&self) -> &str {
        "dummy"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        Err(LlmError::RequestFailed {
            provider: "dummy".to_string(),
            reason: "not implemented".to_string(),
        })
    }
}

struct SmallContextProvider {
    context_window: u32,
}

#[async_trait]
impl LlmProvider for SmallContextProvider {
    fn model_name(&self) -> &str {
        "small-context"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, _request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        Err(LlmError::RequestFailed {
            provider: "small-context".to_string(),
            reason: "not implemented".to_string(),
        })
    }

    fn context_window(&self) -> u32 {
        self.context_window
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

struct FailingCompactor;

#[async_trait]
impl Compactor for FailingCompactor {
    async fn compact(
        &self,
        _provider: &dyn LlmProvider,
        _messages: &[ChatMessage],
        _token_count: u32,
    ) -> Result<Option<CompactResult>, CompactError> {
        Err(CompactError::Failed {
            reason: "boom".to_string(),
        })
    }

    fn name(&self) -> &'static str {
        "failing"
    }
}

fn test_agent_record() -> Arc<AgentRecord> {
    Arc::new(AgentRecord {
        id: AgentId::new(1),
        display_name: "Test Agent".to_string(),
        description: "A test agent".to_string(),
        version: "1.0.0".to_string(),
        provider_id: Some(ProviderId::new(1)),
        model_id: None,
        system_prompt: "You are a test agent.".to_string(),
        tool_names: vec![],
        max_tokens: None,
        temperature: None,
        thinking_config: None,
        parent_agent_id: None,
        agent_type: AgentType::Standard,
    })
}

fn test_agent_record_without_system_prompt() -> Arc<AgentRecord> {
    Arc::new(AgentRecord {
        system_prompt: String::new(),
        ..(*test_agent_record()).clone()
    })
}

#[derive(Debug, Clone)]
enum ResponsePlan {
    Ok(String),
}

#[derive(Debug)]
struct SequencedProvider {
    delay: Duration,
    plans: Arc<Mutex<VecDeque<ResponsePlan>>>,
    captured_user_inputs: Arc<Mutex<Vec<String>>>,
}

impl SequencedProvider {
    fn new(delay: Duration, plans: Vec<ResponsePlan>) -> Self {
        Self {
            delay,
            plans: Arc::new(Mutex::new(VecDeque::from(plans))),
            captured_user_inputs: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn captured_user_inputs(&self) -> Vec<String> {
        self.captured_user_inputs.lock().unwrap().clone()
    }
}

#[async_trait]
impl LlmProvider for SequencedProvider {
    fn model_name(&self) -> &str {
        "sequenced"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let last_user_input = request
            .messages
            .iter()
            .rev()
            .find(|message| message.role == argus_protocol::Role::User)
            .map(|message| message.content.clone())
            .unwrap_or_default();
        self.captured_user_inputs
            .lock()
            .unwrap()
            .push(last_user_input);

        sleep(self.delay).await;

        let next_plan = self
            .plans
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| ResponsePlan::Ok("default response".to_string()));
        let ResponsePlan::Ok(content) = next_plan;
        Ok(CompletionResponse {
            content: Some(content),
            reasoning_content: None,
            tool_calls: Vec::new(),
            input_tokens: 10,
            output_tokens: 5,
            finish_reason: argus_protocol::llm::FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }
}

#[derive(Debug)]
struct BlockingSecondCallProvider {
    plans: Arc<Mutex<VecDeque<ResponsePlan>>>,
    captured_user_inputs: Arc<Mutex<Vec<String>>>,
    second_call_started_tx: Mutex<Option<oneshot::Sender<()>>>,
    release_second_call: Arc<Notify>,
}

impl BlockingSecondCallProvider {
    fn new(
        plans: Vec<ResponsePlan>,
        second_call_started_tx: oneshot::Sender<()>,
        release_second_call: Arc<Notify>,
    ) -> Self {
        Self {
            plans: Arc::new(Mutex::new(VecDeque::from(plans))),
            captured_user_inputs: Arc::new(Mutex::new(Vec::new())),
            second_call_started_tx: Mutex::new(Some(second_call_started_tx)),
            release_second_call,
        }
    }

    fn captured_user_inputs(&self) -> Vec<String> {
        self.captured_user_inputs.lock().unwrap().clone()
    }
}

#[async_trait]
impl LlmProvider for BlockingSecondCallProvider {
    fn model_name(&self) -> &str {
        "blocking-second-call"
    }

    fn cost_per_token(&self) -> (Decimal, Decimal) {
        (Decimal::ZERO, Decimal::ZERO)
    }

    async fn complete(&self, request: CompletionRequest) -> Result<CompletionResponse, LlmError> {
        let last_user_input = request
            .messages
            .iter()
            .rev()
            .find(|message| message.role == argus_protocol::Role::User)
            .map(|message| message.content.clone())
            .unwrap_or_default();
        let call_index = {
            let mut captured = self.captured_user_inputs.lock().unwrap();
            captured.push(last_user_input);
            captured.len()
        };

        if call_index == 2 {
            if let Some(sender) = self.second_call_started_tx.lock().unwrap().take() {
                let _ = sender.send(());
            }
            self.release_second_call.notified().await;
        }

        let next_plan = self
            .plans
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| ResponsePlan::Ok("default response".to_string()));
        let ResponsePlan::Ok(content) = next_plan;
        Ok(CompletionResponse {
            content: Some(content),
            reasoning_content: None,
            tool_calls: Vec::new(),
            input_tokens: 10,
            output_tokens: 5,
            finish_reason: argus_protocol::llm::FinishReason::Stop,
            cache_read_input_tokens: 0,
            cache_creation_input_tokens: 0,
        })
    }
}

fn build_test_thread() -> Thread {
    let compactor: Arc<dyn Compactor> = Arc::new(NoopCompactor);
    ThreadBuilder::new()
        .provider(Arc::new(DummyProvider))
        .compactor(compactor)
        .agent_record(test_agent_record())
        .session_id(SessionId::new())
        .build()
        .expect("thread should build")
}

fn build_test_thread_with_provider(
    provider: Arc<dyn LlmProvider>,
) -> Arc<tokio::sync::RwLock<Thread>> {
    let compactor: Arc<dyn Compactor> = Arc::new(NoopCompactor);
    Arc::new(tokio::sync::RwLock::new(
        ThreadBuilder::new()
            .provider(provider)
            .compactor(compactor)
            .agent_record(test_agent_record())
            .session_id(SessionId::new())
            .build()
            .expect("thread should build"),
    ))
}

fn repeated_test_message() -> String {
    ["test"; 10].join(" ")
}

fn token_count_for_messages(messages: &[ChatMessage]) -> u32 {
    // Simple heuristic: ~1 token per word, similar to the old estimate_tokens.
    messages
        .iter()
        .map(|message| message.content.split_whitespace().count() as u32)
        .sum()
}

async fn wait_for_idle_events(thread: &Arc<tokio::sync::RwLock<Thread>>, expected_count: usize) {
    let mut rx = {
        let guard = thread.read().await;
        guard.subscribe()
    };
    let mut idle_count = 0usize;
    timeout(Duration::from_secs(5), async {
        loop {
            match rx.recv().await {
                Ok(ThreadEvent::Idle { .. }) => {
                    idle_count += 1;
                    if idle_count >= expected_count {
                        break;
                    }
                }
                Ok(_) => {}
                Err(_) => {}
            }
        }
    })
    .await
    .expect("thread should emit idle");
}

async fn collect_runtime_terminal_events(
    rx: &mut broadcast::Receiver<ThreadEvent>,
    expected_count: usize,
) -> Vec<ThreadEvent> {
    let mut events = Vec::with_capacity(expected_count);
    timeout(Duration::from_secs(5), async {
        loop {
            match rx.recv().await {
                Ok(event @ ThreadEvent::TurnCompleted { .. })
                | Ok(event @ ThreadEvent::TurnFailed { .. })
                | Ok(event @ ThreadEvent::TurnSettled { .. })
                | Ok(event @ ThreadEvent::Idle { .. }) => {
                    events.push(event);
                    if events.len() >= expected_count {
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

#[test]
fn thread_builder_requires_provider() {
    let compactor: Arc<dyn Compactor> = Arc::new(NoopCompactor);
    let result = ThreadBuilder::new()
        .compactor(compactor)
        .agent_record(test_agent_record())
        .session_id(SessionId::new())
        .build();
    assert!(matches!(result, Err(ThreadError::ProviderNotConfigured)));
}

#[test]
fn thread_builder_requires_compactor() {
    let result = ThreadBuilder::new()
        .agent_record(test_agent_record())
        .session_id(SessionId::new())
        .build();
    assert!(result.is_err());
}

#[test]
fn thread_builder_requires_agent_record() {
    let compactor: Arc<dyn Compactor> = Arc::new(NoopCompactor);
    let result = ThreadBuilder::new()
        .compactor(compactor)
        .session_id(SessionId::new())
        .build();
    assert!(matches!(result, Err(ThreadError::AgentRecordNotSet)));
}

#[test]
fn thread_builder_requires_session_id() {
    let compactor: Arc<dyn Compactor> = Arc::new(NoopCompactor);
    let result = ThreadBuilder::new()
        .compactor(compactor)
        .agent_record(test_agent_record())
        .build();
    assert!(matches!(result, Err(ThreadError::SessionIdNotSet)));
}

#[test]
fn hydrate_from_persisted_state_preserves_system_prompt_and_updates_runtime_state() {
    let compactor: Arc<dyn Compactor> = Arc::new(NoopCompactor);
    let updated_at = Utc::now();
    let mut thread = ThreadBuilder::new()
        .provider(Arc::new(DummyProvider))
        .compactor(compactor)
        .agent_record(test_agent_record())
        .session_id(SessionId::new())
        .build()
        .unwrap();

    thread.hydrate_from_persisted_state(
        vec![
            ChatMessage::user("历史问题"),
            ChatMessage::assistant("历史回答"),
        ],
        42,
        3,
        updated_at,
    );

    assert_eq!(thread.history().len(), 3);
    assert_eq!(thread.history()[0].role, argus_protocol::llm::Role::System);
    assert_eq!(thread.history()[1].content, "历史问题");
    assert_eq!(thread.history()[2].content, "历史回答");
    assert_eq!(thread.token_count(), 42);
    assert_eq!(thread.turn_count(), 3);
    assert_eq!(thread.updated_at(), updated_at);
}

#[tokio::test]
async fn history_reads_from_cached_committed_messages() {
    let mut thread = build_test_thread();
    thread.hydrate_turn_history_for_test(vec![TurnRecord::completed(
        1,
        vec![ChatMessage::user("hi"), ChatMessage::assistant("hello")],
    )]);

    assert_eq!(thread.history().len(), 3);
    assert_eq!(thread.turn_count(), 1);
}

#[test]
fn shared_history_build_turn_context_prefers_cached_committed_messages() {
    let mut thread = build_test_thread();
    thread.hydrate_turn_history_for_test(vec![TurnRecord::completed(
        1,
        vec![ChatMessage::user("hi"), ChatMessage::assistant("hello")],
    )]);

    thread.messages = Arc::new(vec![
        ChatMessage::system("You are a stale system message."),
        ChatMessage::user("stale"),
        ChatMessage::assistant("history"),
    ]);

    assert_eq!(thread.history()[1].content, "hi");
    assert_eq!(thread.history()[2].content, "hello");

    let context = thread.build_turn_context();
    assert_eq!(context.len(), 3);
    assert_eq!(context[0].role, argus_protocol::llm::Role::System);
    assert_eq!(context[1].content, "hi");
    assert_eq!(context[2].content, "hello");
}

#[tokio::test]
async fn shared_history_turn_reuses_thread_owned_inflight_snapshot() {
    let mut thread = build_test_thread();

    let turn = thread
        .begin_turn("hello".to_string(), None, TurnCancellation::default())
        .await
        .expect("turn should build");

    let current_turn = thread
        .current_turn
        .as_ref()
        .expect("thread should keep an in-flight turn");

    assert_eq!(
        Arc::as_ptr(&current_turn.shared),
        turn.shared_snapshot_ptr()
    );
}

#[tokio::test]
async fn reactor_start_turn_uses_runtime_assigned_turn_number() {
    let thread = Arc::new(RwLock::new(build_test_thread()));
    let mut runtime = ThreadReactor::seeded_from_next_turn_number(7);
    let mut active_turn = None;

    Thread::process_reactor_action(
        Arc::clone(&thread),
        &mut runtime,
        ThreadReactorAction::StartTurn {
            turn_number: 7,
            content: "hello".to_string(),
            msg_override: None,
        },
        &mut active_turn,
    )
    .await;

    let guard = thread.read().await;
    let current_turn = guard
        .current_turn
        .as_ref()
        .expect("runtime should install an in-flight turn");

    assert_eq!(current_turn.turn_number, 7);
    assert_eq!(guard.turn_count(), 7);
}

#[test]
fn builder_keeps_committed_history_and_turn_count_aligned_when_seeded_with_turns() {
    let compactor: Arc<dyn Compactor> = Arc::new(NoopCompactor);
    let thread = ThreadBuilder::new()
        .provider(Arc::new(DummyProvider))
        .compactor(compactor)
        .agent_record(test_agent_record())
        .session_id(SessionId::new())
        .messages(vec![
            ChatMessage::system("You are a stale system message."),
            ChatMessage::user("stale"),
            ChatMessage::assistant("history"),
        ])
        .turns(vec![TurnRecord::completed(
            1,
            vec![ChatMessage::user("hi"), ChatMessage::assistant("hello")],
        )])
        .build()
        .unwrap();

    assert_eq!(thread.history().len(), 3);
    assert_eq!(thread.history()[0].role, argus_protocol::llm::Role::System);
    assert_eq!(thread.history()[1].content, "hi");
    assert_eq!(thread.history()[2].content, "hello");
    assert_eq!(thread.turn_count(), 1);
}

#[test]
fn builder_derives_next_turn_number_from_seeded_turns() {
    let compactor: Arc<dyn Compactor> = Arc::new(NoopCompactor);
    let thread = ThreadBuilder::new()
        .provider(Arc::new(DummyProvider))
        .compactor(compactor)
        .agent_record(test_agent_record())
        .session_id(SessionId::new())
        .turns(vec![TurnRecord::completed(
            4,
            vec![ChatMessage::user("hi"), ChatMessage::assistant("hello")],
        )])
        .build()
        .unwrap();

    assert_eq!(thread.next_turn_number, 5);
}

#[test]
fn plan_returns_read_only_snapshot() {
    let compactor: Arc<dyn Compactor> = Arc::new(NoopCompactor);

    // Create a temp dir and pre-populate plan.json at {temp_dir}/{thread_id}/plan.json
    let temp_dir = std::env::temp_dir()
        .join("argus-thread-test-plan")
        .join("thread-1");
    std::fs::create_dir_all(&temp_dir).unwrap();
    std::fs::write(
        temp_dir.join("plan.json"),
        serde_json::to_string_pretty(&vec![json!({
            "step": "Inspect review feedback",
            "status": "completed"
        })])
        .unwrap(),
    )
    .unwrap();

    let plan_store = FilePlanStore::new(
        std::env::temp_dir().join("argus-thread-test-plan"),
        "thread-1",
    );

    let thread = ThreadBuilder::new()
        .provider(Arc::new(DummyProvider))
        .compactor(compactor)
        .agent_record(test_agent_record())
        .session_id(SessionId::new())
        .plan_store(plan_store)
        .build()
        .unwrap();

    let mut snapshot = thread.plan();
    assert_eq!(
        snapshot,
        vec![json!({
            "step": "Inspect review feedback",
            "status": "completed"
        })]
    );

    snapshot.push(json!({
        "step": "Mutate local copy",
        "status": "pending"
    }));

    assert_eq!(thread.plan().len(), 1);
    assert_eq!(thread.info().plan_item_count, 1);
}

#[test]
fn thread_handle_enqueue_tracks_pending_queue_depth_while_running() {
    let mut handle = ThreadHandle::new();

    let first = handle.dispatch(ThreadCommand::EnqueueUserMessage {
        content: "first".to_string(),
        msg_override: None,
    });
    assert!(matches!(
        first,
        ThreadReactorAction::StartTurn { turn_number: 1, .. }
    ));

    let second = handle.dispatch(ThreadCommand::EnqueueUserMessage {
        content: "second".to_string(),
        msg_override: None,
    });
    assert!(matches!(second, ThreadReactorAction::Noop));

    let snapshot = handle.snapshot();
    assert_eq!(
        snapshot.state,
        ThreadRuntimeState::Running { turn_number: 1 }
    );
    assert_eq!(snapshot.queue_depth, 1);
}

#[test]
fn thread_reactor_transitions_idle_to_running_and_back_to_idle() {
    let mut reactor = ThreadReactor::default();
    let mut mailbox = ThreadMailbox::default();

    let start = reactor.apply_command(
        ThreadCommand::EnqueueUserMessage {
            content: "hello".to_string(),
            msg_override: None,
        },
        &mut mailbox,
    );
    assert!(matches!(
        start,
        ThreadReactorAction::StartTurn { turn_number: 1, .. }
    ));
    assert_eq!(
        reactor.state(),
        ThreadRuntimeState::Running { turn_number: 1 }
    );

    let finish = reactor.finish_active_turn(&mut mailbox);
    assert!(matches!(finish, ThreadReactorAction::Noop));
    assert_eq!(reactor.state(), ThreadRuntimeState::Idle);
}

#[tokio::test]
async fn cancelled_or_completed_turn_starts_next_queued_message() {
    let provider = Arc::new(SequencedProvider::new(
        Duration::from_millis(120),
        vec![
            ResponsePlan::Ok("first turn reply".to_string()),
            ResponsePlan::Ok("second turn reply".to_string()),
        ],
    ));
    let thread = build_test_thread_with_provider(provider.clone());

    Thread::spawn_reactor(Arc::clone(&thread));

    {
        let guard = thread.read().await;
        guard
            .send_user_message("first queued".to_string(), None)
            .expect("first message should queue");
    }

    sleep(Duration::from_millis(20)).await;

    {
        let guard = thread.read().await;
        guard
            .send_user_message("second queued".to_string(), None)
            .expect("second message should queue");
    }

    sleep(Duration::from_millis(30)).await;
    {
        let guard = thread.read().await;
        assert_eq!(guard.turn_count(), 1);
    }
    assert_eq!(provider.captured_user_inputs().len(), 1);

    wait_for_idle_events(&thread, 1).await;

    let captured = provider.captured_user_inputs();
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0], "first queued");
    assert_eq!(captured[1], "second queued");
}

#[tokio::test]
async fn reactor_emits_turn_settled_before_idle_after_completed_turn() {
    let provider = Arc::new(SequencedProvider::new(
        Duration::from_millis(20),
        vec![ResponsePlan::Ok("settled reply".to_string())],
    ));
    let thread = build_test_thread_with_provider(provider);

    Thread::spawn_reactor(Arc::clone(&thread));
    let mut rx = {
        let guard = thread.read().await;
        guard.subscribe()
    };

    {
        let guard = thread.read().await;
        guard
            .send_user_message("settle this turn".to_string(), None)
            .expect("message should queue");
    }

    let events = collect_runtime_terminal_events(&mut rx, 3).await;
    assert!(matches!(events[0], ThreadEvent::TurnCompleted { .. }));
    assert!(matches!(events[1], ThreadEvent::TurnSettled { .. }));
    assert!(matches!(events[2], ThreadEvent::Idle { .. }));

    let guard = thread.read().await;
    assert_eq!(guard.turn_count(), 1);
    assert!(guard.token_count() > 0);
}

#[tokio::test]
async fn reactor_emits_turn_settled_before_idle_after_failed_turn() {
    let thread = build_test_thread_with_provider(Arc::new(DummyProvider));

    Thread::spawn_reactor(Arc::clone(&thread));
    let mut rx = {
        let guard = thread.read().await;
        guard.subscribe()
    };

    {
        let guard = thread.read().await;
        guard
            .send_user_message("fail this turn".to_string(), None)
            .expect("message should queue");
    }

    let events = collect_runtime_terminal_events(&mut rx, 3).await;
    assert!(matches!(events[0], ThreadEvent::TurnFailed { .. }));
    assert!(matches!(events[1], ThreadEvent::TurnSettled { .. }));
    assert!(matches!(events[2], ThreadEvent::Idle { .. }));
}

#[tokio::test]
async fn reactor_does_not_start_follow_up_turn_before_prior_settlement() {
    let (second_call_started_tx, mut second_call_started_rx) = oneshot::channel();
    let release_second_call = Arc::new(Notify::new());
    let provider = Arc::new(BlockingSecondCallProvider::new(
        vec![
            ResponsePlan::Ok("first turn reply".to_string()),
            ResponsePlan::Ok("second turn reply".to_string()),
        ],
        second_call_started_tx,
        Arc::clone(&release_second_call),
    ));
    let thread = build_test_thread_with_provider(provider.clone());

    Thread::spawn_reactor(Arc::clone(&thread));

    let mut rx = {
        let guard = thread.read().await;
        guard.subscribe()
    };

    {
        let guard = thread.read().await;
        guard
            .send_user_message("first queued".to_string(), None)
            .expect("first message should queue");
    }

    sleep(Duration::from_millis(20)).await;

    {
        let guard = thread.read().await;
        guard
            .send_user_message("second queued".to_string(), None)
            .expect("second message should queue");
    }

    let first_terminal_turn = timeout(Duration::from_secs(5), async {
        loop {
            tokio::select! {
                event = rx.recv() => {
                    match event {
                        Ok(ThreadEvent::TurnCompleted { turn_number, .. })
                        | Ok(ThreadEvent::TurnFailed { turn_number, .. }) => break turn_number,
                        Ok(_) => {}
                        Err(_) => {}
                    }
                }
                started = &mut second_call_started_rx => {
                    started.expect("second call start signal should stay open");
                    panic!("queued follow-up work must not start before the first turn settles");
                }
            }
        }
    })
    .await
    .expect("thread should emit a terminal turn event");

    timeout(Duration::from_secs(5), async {
        loop {
            match rx.recv().await {
                Ok(ThreadEvent::TurnSettled { turn_number, .. })
                    if turn_number == first_terminal_turn =>
                {
                    break;
                }
                Ok(_) => {}
                Err(_) => {}
            }
        }
    })
    .await
    .expect("first turn should settle");

    release_second_call.notify_one();
    wait_for_idle_events(&thread, 1).await;

    let captured = provider.captured_user_inputs();
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0], "first queued");
    assert_eq!(captured[1], "second queued");
}

#[tokio::test]
async fn user_interrupt_cancels_active_turn_and_preserves_queue() {
    let provider = Arc::new(SequencedProvider::new(
        Duration::from_millis(120),
        vec![
            ResponsePlan::Ok("first turn reply".to_string()),
            ResponsePlan::Ok("second turn reply".to_string()),
        ],
    ));
    let thread = build_test_thread_with_provider(provider.clone());

    Thread::spawn_reactor(Arc::clone(&thread));

    {
        let guard = thread.read().await;
        guard
            .send_user_message("first queued".to_string(), None)
            .expect("first message should queue");
    }

    sleep(Duration::from_millis(20)).await;

    {
        let guard = thread.read().await;
        guard
            .send_user_message("second queued".to_string(), None)
            .expect("second message should queue");
    }

    sleep(Duration::from_millis(20)).await;

    {
        let guard = thread.read().await;
        guard
            .send_control_event(ThreadControlEvent::UserInterrupt {
                content: "stop".to_string(),
            })
            .expect("interrupt should request stop");
    }

    wait_for_idle_events(&thread, 1).await;

    let captured = provider.captured_user_inputs();
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0], "first queued");
    assert_eq!(captured[1], "second queued");

    let assistant_count = {
        let guard = thread.read().await;
        guard
            .history()
            .iter()
            .filter(|message| message.role == argus_protocol::llm::Role::Assistant)
            .count()
    };

    assert_eq!(
        assistant_count, 1,
        "cancelled first turn should not append assistant output",
    );

    let (user_messages, assistant_messages) = {
        let guard = thread.read().await;
        let user_messages = guard
            .history()
            .iter()
            .filter(|message| message.role == argus_protocol::llm::Role::User)
            .map(|message| message.content.clone())
            .collect::<Vec<_>>();
        let assistant_messages = guard
            .history()
            .iter()
            .filter(|message| message.role == argus_protocol::llm::Role::Assistant)
            .map(|message| message.content.clone())
            .collect::<Vec<_>>();
        (user_messages, assistant_messages)
    };

    assert_eq!(
        user_messages,
        vec!["first queued".to_string(), "second queued".to_string()],
        "stop should preserve the user bubbles that were already sent",
    );
    assert_eq!(
        assistant_messages.len(),
        1,
        "stop should discard only the cancelled turn's assistant output",
    );
}

#[tokio::test]
async fn legacy_mailbox_interrupt_does_not_leak_into_next_turn_after_idle_handoff() {
    let provider = Arc::new(SequencedProvider::new(
        Duration::from_millis(120),
        vec![
            ResponsePlan::Ok("first turn reply".to_string()),
            ResponsePlan::Ok("second turn reply".to_string()),
        ],
    ));
    let thread = build_test_thread_with_provider(provider.clone());

    Thread::spawn_reactor(Arc::clone(&thread));

    {
        let guard = thread.read().await;
        guard
            .send_user_message("first queued".to_string(), None)
            .expect("first message should queue");
    }

    sleep(Duration::from_millis(20)).await;

    {
        let mailbox = {
            let guard = thread.read().await;
            guard.mailbox()
        };

        let mut guard = mailbox.lock().await;
        guard.push(ThreadControlEvent::UserInterrupt {
            content: "late interrupt".to_string(),
        });
    }

    wait_for_idle_events(&thread, 1).await;

    {
        let guard = thread.read().await;
        guard
            .send_user_message("second queued".to_string(), None)
            .expect("second message should queue");
    }

    wait_for_idle_events(&thread, 1).await;

    let captured = provider.captured_user_inputs();
    assert_eq!(captured.len(), 2);
    assert_eq!(captured[0], "first queued");
    assert_eq!(
        captured[1], "second queued",
        "late interrupt should be cleared on idle handoff",
    );
}

/// A compactor that always returns a single-user-message result.
struct SingleMessageCompactor;

#[async_trait]
impl Compactor for SingleMessageCompactor {
    async fn compact(
        &self,
        _provider: &dyn LlmProvider,
        _messages: &[ChatMessage],
        _token_count: u32,
    ) -> Result<Option<CompactResult>, CompactError> {
        Ok(Some(CompactResult {
            summary_messages: vec![ChatMessage::user("compacted")],
            token_count: 10,
        }))
    }

    fn name(&self) -> &'static str {
        "single_message"
    }
}

#[tokio::test]
async fn begin_turn_broadcasts_compacted_event_when_pre_turn_compaction_changes_history() {
    let compactor: Arc<dyn Compactor> = Arc::new(SingleMessageCompactor);
    let mut thread = ThreadBuilder::new()
        .provider(Arc::new(SmallContextProvider {
            context_window: 100,
        }))
        .compactor(compactor)
        .agent_record(test_agent_record_without_system_prompt())
        .session_id(SessionId::new())
        .build()
        .expect("thread should build");
    let repeated = repeated_test_message();
    let persisted_messages = vec![
        ChatMessage::user(repeated.clone()),
        ChatMessage::user(repeated.clone()),
        ChatMessage::user(repeated.clone()),
    ];
    thread.hydrate_from_persisted_state(
        persisted_messages.clone(),
        token_count_for_messages(&persisted_messages),
        0,
        Utc::now(),
    );

    let mut rx = thread.subscribe();
    let _turn = thread
        .begin_turn("next".to_string(), None, TurnCancellation::new())
        .await
        .expect("turn should build after compaction");

    let event = timeout(Duration::from_millis(100), rx.recv())
        .await
        .expect("thread should emit compacted event")
        .expect("event should be readable");
    assert!(matches!(
        event,
        ThreadEvent::Compacted {
            new_token_count: 10,
            ..
        }
    ));
}

#[tokio::test]
async fn compaction_does_not_rewrite_committed_turn_history() {
    let compactor: Arc<dyn Compactor> = Arc::new(SingleMessageCompactor);
    let mut thread = ThreadBuilder::new()
        .provider(Arc::new(SmallContextProvider {
            context_window: 100,
        }))
        .compactor(compactor)
        .agent_record(test_agent_record_without_system_prompt())
        .session_id(SessionId::new())
        .build()
        .expect("thread should build");
    let repeated = repeated_test_message();
    let persisted_messages = vec![
        ChatMessage::user(repeated.clone()),
        ChatMessage::assistant("history reply"),
        ChatMessage::user(repeated),
    ];
    thread.hydrate_from_persisted_state(persisted_messages, 90, 0, Utc::now());
    let before = thread.history().to_vec();

    let _turn = thread
        .begin_turn("follow-up".to_string(), None, TurnCancellation::default())
        .await
        .expect("turn should build after compaction");

    let after = thread.history().to_vec();
    assert_eq!(after.len(), before.len());
    for (after_message, before_message) in after.iter().zip(before.iter()) {
        assert_eq!(after_message.role, before_message.role);
        assert_eq!(after_message.content, before_message.content);
    }
}

#[tokio::test]
async fn build_turn_context_uses_compaction_checkpoint_summary_messages() {
    let compactor: Arc<dyn Compactor> = Arc::new(SingleMessageCompactor);
    let mut thread = ThreadBuilder::new()
        .provider(Arc::new(SmallContextProvider {
            context_window: 100,
        }))
        .compactor(compactor)
        .agent_record(test_agent_record_without_system_prompt())
        .session_id(SessionId::new())
        .build()
        .expect("thread should build");
    let persisted_messages = vec![
        ChatMessage::user("old question"),
        ChatMessage::assistant("old answer"),
    ];
    thread.hydrate_from_persisted_state(persisted_messages, 90, 0, Utc::now());

    let _turn = thread
        .begin_turn("follow-up".to_string(), None, TurnCancellation::default())
        .await
        .expect("turn should build after compaction");

    let context = thread.build_turn_context();
    assert_eq!(context.len(), 1);
    assert_eq!(context[0].content, "compacted");
}

#[tokio::test]
async fn begin_turn_does_not_broadcast_compacted_event_when_history_is_unchanged() {
    let compactor: Arc<dyn Compactor> = Arc::new(NoopCompactor);
    let mut thread = ThreadBuilder::new()
        .provider(Arc::new(SmallContextProvider {
            context_window: 100,
        }))
        .compactor(compactor)
        .agent_record(test_agent_record_without_system_prompt())
        .session_id(SessionId::new())
        .build()
        .expect("thread should build");
    let persisted_messages = vec![ChatMessage::user(repeated_test_message())];
    thread.hydrate_from_persisted_state(
        persisted_messages.clone(),
        token_count_for_messages(&persisted_messages),
        0,
        Utc::now(),
    );

    let mut rx = thread.subscribe();
    let _turn = thread
        .begin_turn("next".to_string(), None, TurnCancellation::new())
        .await
        .expect("turn should build without compaction");

    let no_event = timeout(Duration::from_millis(50), rx.recv()).await;
    assert!(
        no_event.is_err(),
        "unchanged history should not emit compacted event",
    );
}

#[tokio::test]
async fn begin_turn_ignores_compaction_failure_and_does_not_emit_compacted_event() {
    let mut thread = ThreadBuilder::new()
        .provider(Arc::new(SmallContextProvider {
            context_window: 100,
        }))
        .compactor(Arc::new(FailingCompactor))
        .agent_record(test_agent_record_without_system_prompt())
        .session_id(SessionId::new())
        .build()
        .expect("thread should build");
    let repeated = repeated_test_message();
    let persisted_messages = vec![
        ChatMessage::user(repeated.clone()),
        ChatMessage::user(repeated),
    ];
    thread.hydrate_from_persisted_state(
        persisted_messages.clone(),
        token_count_for_messages(&persisted_messages),
        0,
        Utc::now(),
    );

    let mut rx = thread.subscribe();
    let _turn = thread
        .begin_turn("next".to_string(), None, TurnCancellation::new())
        .await
        .expect("turn should still build after compact failure");

    let no_event = timeout(Duration::from_millis(50), rx.recv()).await;
    assert!(
        no_event.is_err(),
        "failed compaction should not emit compacted event",
    );
}

#[tokio::test]
async fn begin_turn_preserves_authoritative_token_count_until_visible_turn_completes() {
    let mut thread = ThreadBuilder::new()
        .provider(Arc::new(SmallContextProvider {
            context_window: 4_096,
        }))
        .compactor(Arc::new(NoopCompactor))
        .agent_record(test_agent_record_without_system_prompt())
        .session_id(SessionId::new())
        .build()
        .expect("thread should build");
    let persisted_messages = vec![ChatMessage::user(repeated_test_message())];
    let next_message = "next message".to_string();
    let authoritative_token_count = token_count_for_messages(&persisted_messages);
    thread.hydrate_from_persisted_state(
        persisted_messages.clone(),
        authoritative_token_count,
        0,
        Utc::now(),
    );

    let _turn = thread
        .begin_turn(next_message.clone(), None, TurnCancellation::new())
        .await
        .expect("turn should build");

    assert_eq!(thread.token_count(), authoritative_token_count);
}

#[tokio::test]
async fn finish_turn_moves_current_turn_into_turns() {
    let mut thread = build_test_thread();
    let turn = thread
        .begin_turn("hi".to_string(), None, TurnCancellation::default())
        .await
        .expect("turn should build");

    let output = TurnOutput {
        appended_messages: vec![ChatMessage::assistant("hello")],
        token_usage: argus_protocol::TokenUsage {
            input_tokens: 1,
            output_tokens: 1,
            total_tokens: 2,
        },
    };

    assert_eq!(thread.history().len(), 1);
    assert_eq!(thread.history()[0].content, "You are a test agent.");
    assert!(thread.current_turn.is_some());
    assert_eq!(thread.turns.len(), 0);

    drop(turn);
    thread.finish_turn(Ok(output)).expect("turn should settle");

    assert!(thread.current_turn.is_none());
    assert_eq!(thread.turn_count(), 1);
    assert_eq!(thread.turns.len(), 1);
    assert_eq!(thread.turns[0].messages.len(), 2);
    assert_eq!(thread.turns[0].messages[0].content, "hi");
    assert_eq!(thread.turns[0].messages[1].content, "hello");
}

#[tokio::test]
async fn finish_turn_cancelled_preserves_last_authoritative_token_count() {
    let mut thread = ThreadBuilder::new()
        .provider(Arc::new(SmallContextProvider {
            context_window: 4_096,
        }))
        .compactor(Arc::new(NoopCompactor))
        .agent_record(test_agent_record_without_system_prompt())
        .session_id(SessionId::new())
        .build()
        .expect("thread should build");
    let persisted_messages = vec![ChatMessage::user(repeated_test_message())];
    let next_message = "cancel me".to_string();
    let authoritative_token_count = token_count_for_messages(&persisted_messages);
    thread.hydrate_from_persisted_state(
        persisted_messages.clone(),
        authoritative_token_count,
        0,
        Utc::now(),
    );

    let _turn = thread
        .begin_turn(next_message.clone(), None, TurnCancellation::new())
        .await
        .expect("turn should build");
    thread
        .finish_turn(Err(ThreadError::TurnFailed(crate::TurnError::Cancelled)))
        .expect("cancelled turn should be ignored");

    assert_eq!(thread.token_count(), authoritative_token_count);
    assert_eq!(thread.turns.len(), 1);
    assert!(matches!(
        thread.turns[0].state,
        crate::history::TurnState::Cancelled
    ));
    assert_eq!(thread.turns[0].messages.len(), 1);
    assert_eq!(thread.turns[0].messages[0].content, next_message);
}

#[tokio::test]
async fn finish_turn_preserves_legacy_turn_count_bridge() {
    let mut thread = ThreadBuilder::new()
        .provider(Arc::new(DummyProvider))
        .compactor(Arc::new(NoopCompactor))
        .agent_record(test_agent_record_without_system_prompt())
        .session_id(SessionId::new())
        .build()
        .expect("thread should build");
    let persisted_messages = vec![ChatMessage::user("legacy")];
    thread.hydrate_from_persisted_state(persisted_messages, 1, 5, Utc::now());

    let _turn = thread
        .begin_turn("next".to_string(), None, TurnCancellation::new())
        .await
        .expect("turn should build");
    thread
        .finish_turn(Ok(TurnOutput {
            appended_messages: vec![ChatMessage::user("next"), ChatMessage::assistant("done")],
            token_usage: argus_protocol::TokenUsage {
                input_tokens: 1,
                output_tokens: 1,
                total_tokens: 2,
            },
        }))
        .expect("turn should settle");

    assert_eq!(thread.turn_count(), 6);
}

#[tokio::test]
async fn turn_log_persistence_snapshot_writes_messages_and_meta() {
    let temp_dir = tempfile::tempdir().expect("temp dir should exist");
    let session_id = SessionId::new();
    let trace_config =
        TraceConfig::new(true, temp_dir.path().to_path_buf()).with_session_id(session_id);
    let thread_config = ThreadConfig {
        turn_config: TurnConfigBuilder::default()
            .trace_config(trace_config)
            .build()
            .expect("turn config should build"),
    };
    let mut thread = ThreadBuilder::new()
        .provider(Arc::new(DummyProvider))
        .compactor(Arc::new(NoopCompactor))
        .agent_record(test_agent_record_without_system_prompt())
        .session_id(session_id)
        .config(thread_config)
        .build()
        .expect("thread should build");

    let _turn = thread
        .begin_turn("hi".to_string(), None, TurnCancellation::default())
        .await
        .expect("turn should build");
    thread
        .finish_turn(Ok(TurnOutput {
            appended_messages: vec![ChatMessage::assistant("hello")],
            token_usage: argus_protocol::TokenUsage {
                input_tokens: 1,
                output_tokens: 1,
                total_tokens: 2,
            },
        }))
        .expect("turn should settle");

    let snapshot = thread
        .turn_log_persistence_snapshot()
        .expect("trace-enabled thread should expose persistence snapshot");
    persist_turn_log_snapshot(&snapshot)
        .await
        .expect("snapshot should persist");

    let persisted_turns_dir = turns_dir(&snapshot.base_dir);
    let messages = read_turn_messages(&turn_messages_path(&persisted_turns_dir, 1))
        .await
        .expect("messages should read");
    let meta = read_turn_meta(&turn_meta_path(&persisted_turns_dir, 1))
        .await
        .expect("meta should read");

    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].content, "hi");
    assert_eq!(messages[1].content, "hello");
    assert_eq!(meta.turn_number, 1);
    assert!(matches!(meta.state, crate::history::TurnState::Completed));
    assert_eq!(
        meta.token_usage.as_ref().map(|usage| usage.total_tokens),
        Some(2)
    );
}

#[tokio::test]
async fn recovered_turn_log_state_restores_checkpoint_context_and_next_turn_number() {
    let temp_dir = tempfile::tempdir().expect("temp dir should exist");
    let base_dir = temp_dir.path().join("session").join("thread");
    let system_messages = vec![ChatMessage::system("Persisted system prompt")];
    let checkpoint = CompactionCheckpoint {
        summarized_through_turn: 1,
        summary_messages: vec![ChatMessage::assistant("summary of turn one")],
        created_at: Utc::now(),
    };
    let turn_one = TurnRecord {
        turn_number: 1,
        state: crate::history::TurnState::Completed,
        messages: vec![
            ChatMessage::user("old question"),
            ChatMessage::assistant("old answer"),
        ],
        token_usage: Some(argus_protocol::TokenUsage {
            input_tokens: 4,
            output_tokens: 6,
            total_tokens: 10,
        }),
        started_at: Utc::now(),
        finished_at: Some(Utc::now()),
        model: Some("dummy".to_string()),
        error: None,
    };
    let turn_two = TurnRecord {
        turn_number: 2,
        state: crate::history::TurnState::Completed,
        messages: vec![
            ChatMessage::user("latest question"),
            ChatMessage::assistant("latest answer"),
        ],
        token_usage: Some(argus_protocol::TokenUsage {
            input_tokens: 10,
            output_tokens: 12,
            total_tokens: 22,
        }),
        started_at: Utc::now(),
        finished_at: Some(Utc::now()),
        model: Some("dummy".to_string()),
        error: None,
    };

    persist_turn_log_snapshot(&TurnLogPersistenceSnapshot {
        base_dir: base_dir.clone(),
        turn: turn_one,
        system_messages: system_messages.clone(),
        checkpoint: None,
    })
    .await
    .expect("first turn log snapshot should persist");
    persist_turn_log_snapshot(&TurnLogPersistenceSnapshot {
        base_dir: base_dir.clone(),
        turn: turn_two,
        system_messages: system_messages.clone(),
        checkpoint: Some(checkpoint.clone()),
    })
    .await
    .expect("second turn log snapshot should persist");

    let recovered = recover_thread_log_state(&base_dir, Some(2))
        .await
        .expect("turn log state should recover");

    let mut thread = ThreadBuilder::new()
        .provider(Arc::new(DummyProvider))
        .compactor(Arc::new(NoopCompactor))
        .agent_record(test_agent_record())
        .session_id(SessionId::new())
        .build()
        .expect("thread should build");
    thread.hydrate_from_turn_log_state(recovered, Utc::now());

    assert_eq!(thread.history().len(), 5);
    assert_eq!(thread.history()[0].content, "Persisted system prompt");
    assert_eq!(thread.history()[1].content, "old question");
    assert_eq!(thread.history()[4].content, "latest answer");
    assert_eq!(thread.turn_count(), 2);
    assert_eq!(thread.token_count(), 22);

    let context = thread.build_turn_context();
    assert_eq!(context.len(), 4);
    assert_eq!(context[0].content, "Persisted system prompt");
    assert_eq!(context[1].content, "summary of turn one");
    assert_eq!(context[2].content, "latest question");
    assert_eq!(context[3].content, "latest answer");

    let _turn = thread
        .begin_turn("follow-up".to_string(), None, TurnCancellation::default())
        .await
        .expect("recovered thread should start the next turn");
    let current_turn = thread
        .current_turn
        .as_ref()
        .expect("recovered thread should install an in-flight turn");

    assert_eq!(current_turn.turn_number, 3);
    assert_eq!(current_turn.shared.history.len(), 4);
    assert_eq!(
        current_turn.shared.history[0].content,
        "Persisted system prompt"
    );
    assert_eq!(
        current_turn.shared.history[1].content,
        "summary of turn one"
    );
    assert_eq!(current_turn.shared.history[2].content, "latest question");
    assert_eq!(current_turn.shared.history[3].content, "latest answer");
}

#[tokio::test]
async fn finish_turn_failure_settles_failed_turn_and_returns_error() {
    let mut thread = build_test_thread();
    let _turn = thread
        .begin_turn("hi".to_string(), None, TurnCancellation::new())
        .await
        .expect("turn should build");

    let result = thread.finish_turn(Err(ThreadError::TurnFailed(
        crate::TurnError::ToolExecutionFailed {
            name: "search".to_string(),
            reason: "boom".to_string(),
        },
    )));

    assert!(matches!(result, Err(ThreadError::TurnFailed(_))));
    assert_eq!(thread.turns.len(), 1);
    assert!(matches!(
        thread.turns[0].state,
        crate::history::TurnState::Failed
    ));
    assert_eq!(thread.turns[0].messages.len(), 1);
    assert_eq!(thread.turns[0].messages[0].content, "hi");
    assert_eq!(thread.token_count(), 0);
}
