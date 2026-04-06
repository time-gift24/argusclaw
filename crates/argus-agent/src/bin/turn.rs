//! argus-turn CLI - Test Turn execution with tools and hooks.
//!
//! Usage:
//!   argus-turn execute --prompt "Hello" [--stream]
//!   argus-turn tool-test --prompt "What is 2+2?"
//!   argus-turn compact-test [--rounds 5] [--threshold-ratio 0.1]

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use clap::{Args, Parser, Subcommand};
use serde::Deserialize;
use tokio::sync::broadcast;

use argus_agent::{LlmThreadCompactor, Thread, ThreadBuilder, TurnCancellation};
use argus_llm::providers::{
    OpenAiCompatibleConfig, OpenAiCompatibleFactoryConfig, create_openai_compatible_provider,
};
use argus_llm::retry::{RetryConfig, RetryProvider};
use argus_protocol::llm::{LlmProvider, Role, ToolDefinition};
use argus_protocol::tool::{NamedTool, ToolError, ToolExecutionContext};
use argus_protocol::{AgentId, AgentRecord, AgentType, SessionId, ThreadEvent};
use argus_tool::ToolManager;

/// Configuration file structure.
#[derive(Debug, Deserialize, Default)]
struct Config {
    base_url: Option<String>,
    api_key: Option<String>,
    model: Option<String>,
}

impl Config {
    /// Load configuration from file.
    fn load(path: &PathBuf) -> Self {
        match std::fs::read_to_string(path) {
            Ok(contents) => toml::from_str(&contents).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }
}

/// Resolved configuration from all sources.
#[derive(Debug, Clone)]
struct ResolvedConfig {
    base_url: String,
    api_key: String,
    model: String,
}

#[derive(Parser)]
#[command(
    name = "argus-turn",
    version,
    about = "Test Turn execution with tools and hooks"
)]
struct Cli {
    /// Configuration file path (default: ./turn.toml)
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Execute a turn with optional tools.
    Execute(ExecuteArgs),
    /// Test tool execution.
    ToolTest(ToolTestArgs),
    /// Test retry behavior with mock providers.
    MockTest(MockTestArgs),
    /// Test context compaction by running multi-turn conversations.
    CompactTest(CompactTestArgs),
}

#[derive(Args)]
struct ExecuteArgs {
    /// API base URL
    #[arg(long, env = "ARGUS_LLM_BASE_URL")]
    base_url: Option<String>,
    /// API key
    #[arg(long, env = "ARGUS_LLM_API_KEY")]
    api_key: Option<String>,
    /// Model name
    #[arg(long, env = "ARGUS_LLM_MODEL")]
    model: Option<String>,
    /// The prompt to execute
    #[arg(long)]
    prompt: String,
    /// Enable streaming output
    #[arg(long, default_value_t = false)]
    stream: bool,
}

#[derive(Args)]
struct ToolTestArgs {
    /// API base URL
    #[arg(long, env = "ARGUS_LLM_BASE_URL")]
    base_url: Option<String>,
    /// API key
    #[arg(long, env = "ARGUS_LLM_API_KEY")]
    api_key: Option<String>,
    /// Model name
    #[arg(long, env = "ARGUS_LLM_MODEL")]
    model: Option<String>,
    /// The prompt to execute
    #[arg(long)]
    prompt: String,
}

#[derive(Args)]
struct MockTestArgs {
    /// Test type: "intermittent" or "always-fail"
    #[arg(long)]
    test_type: String,
    /// Maximum number of retries
    #[arg(long, default_value_t = 3)]
    max_retries: u32,
    /// Enable streaming
    #[arg(long, default_value_t = false)]
    stream: bool,
}

#[derive(Args)]
struct CompactTestArgs {
    /// API base URL
    #[arg(long, env = "ARGUS_LLM_BASE_URL")]
    base_url: Option<String>,
    /// API key
    #[arg(long, env = "ARGUS_LLM_API_KEY")]
    api_key: Option<String>,
    /// Model name
    #[arg(long, env = "ARGUS_LLM_MODEL")]
    model: Option<String>,
    /// Number of conversation rounds to run
    #[arg(long, default_value_t = 5)]
    rounds: usize,
    /// Compaction threshold ratio (0.1-0.95). Lower = triggers sooner.
    #[arg(long, default_value_t = 0.1)]
    threshold_ratio: f32,
    /// Enable streaming output
    #[arg(long, default_value_t = false)]
    stream: bool,
}

/// Echo tool for testing
struct EchoTool;

#[async_trait]
impl NamedTool for EchoTool {
    fn name(&self) -> &str {
        "echo"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "echo".to_string(),
            description: "Echo back the input message".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "message": {
                        "type": "string",
                        "description": "The message to echo back"
                    }
                },
                "required": ["message"]
            }),
        }
    }

    async fn execute(
        &self,
        input: serde_json::Value,
        _ctx: Arc<ToolExecutionContext>,
    ) -> Result<serde_json::Value, ToolError> {
        let message = input
            .get("message")
            .and_then(|v| v.as_str())
            .ok_or_else(|| ToolError::ExecutionFailed {
                tool_name: "echo".to_string(),
                reason: "Missing 'message' parameter".to_string(),
            })?;

        Ok(serde_json::json!({
            "echoed": message
        }))
    }
}

fn create_provider(config: &ResolvedConfig) -> Result<Arc<dyn LlmProvider>> {
    let openai_config = OpenAiCompatibleConfig::new(
        config.base_url.clone(),
        config.api_key.clone(),
        config.model.clone(),
    );
    let factory_config = OpenAiCompatibleFactoryConfig::new(openai_config);

    create_openai_compatible_provider(factory_config)
        .map_err(|e| anyhow!("Failed to create provider: {}", e))
}

fn resolve_config(
    config: &Config,
    base_url: Option<&str>,
    api_key: Option<&str>,
    model: Option<&str>,
) -> Result<ResolvedConfig> {
    Ok(ResolvedConfig {
        base_url: base_url
            .map(|s| s.to_string())
            .or_else(|| std::env::var("ARGUS_LLM_BASE_URL").ok())
            .or_else(|| config.base_url.clone())
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
        api_key: api_key
            .map(|s| s.to_string())
            .or_else(|| std::env::var("ARGUS_LLM_API_KEY").ok())
            .or_else(|| config.api_key.clone())
            .ok_or_else(|| {
                anyhow!("API key is required (set --api-key, ARGUS_LLM_API_KEY, or config file)")
            })?,
        model: model
            .map(|s| s.to_string())
            .or_else(|| std::env::var("ARGUS_LLM_MODEL").ok())
            .or_else(|| config.model.clone())
            .unwrap_or_else(|| "gpt-4o-mini".to_string()),
    })
}

fn build_cli_agent_record(
    system_prompt: impl Into<String>,
    tool_names: Vec<&str>,
) -> Arc<AgentRecord> {
    Arc::new(AgentRecord {
        id: AgentId::new(0),
        display_name: "argus-turn".to_string(),
        description: "CLI thread wrapper for one-shot turn execution".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        provider_id: None,
        model_id: None,
        system_prompt: system_prompt.into(),
        tool_names: tool_names.into_iter().map(str::to_string).collect(),
        max_tokens: None,
        temperature: None,
        thinking_config: None,
        parent_agent_id: None,
        agent_type: AgentType::Standard,
    })
}

fn build_thread(
    provider: Arc<dyn LlmProvider>,
    agent_record: Arc<AgentRecord>,
    tool_manager: Option<Arc<ToolManager>>,
) -> Result<Thread> {
    let builder = ThreadBuilder::new()
        .provider(Arc::clone(&provider))
        .compactor(Arc::new(LlmThreadCompactor::new(provider)))
        .agent_record(agent_record)
        .session_id(SessionId::new());
    let builder = if let Some(tool_manager) = tool_manager {
        builder.tool_manager(tool_manager)
    } else {
        builder
    };

    builder
        .build()
        .map_err(|e| anyhow!("Failed to build thread: {}", e))
}

fn build_compact_thread(provider: Arc<dyn LlmProvider>, threshold_ratio: f32) -> Result<Thread> {
    let compactor =
        LlmThreadCompactor::new(Arc::clone(&provider)).with_threshold_ratio(threshold_ratio);
    ThreadBuilder::new()
        .provider(Arc::clone(&provider))
        .compactor(Arc::new(compactor))
        .agent_record(build_cli_agent_record(
            "You are a helpful assistant. Respond concisely.",
            vec![],
        ))
        .session_id(SessionId::new())
        .build()
        .map_err(|e| anyhow!("Failed to build thread: {}", e))
}

fn collect_compaction_events(
    mut rx: broadcast::Receiver<ThreadEvent>,
) -> tokio::task::JoinHandle<Vec<ThreadEvent>> {
    tokio::spawn(async move {
        let mut events = Vec::new();
        loop {
            match rx.recv().await {
                Ok(event) => match &event {
                    ThreadEvent::Compacted { .. }
                    | ThreadEvent::CompactionStarted { .. }
                    | ThreadEvent::CompactionFinished { .. }
                    | ThreadEvent::CompactionFailed { .. } => events.push(event),
                    _ => {}
                },
                Err(broadcast::error::RecvError::Closed) => break,
                Err(broadcast::error::RecvError::Lagged(_)) => continue,
            }
        }
        events
    })
}

/// Long prompt template to quickly fill up the context window.
fn generate_round_prompt(round: usize) -> String {
    let topic = match round % 5 {
        0 => {
            "Explain the architecture of a microservices system in detail, including service discovery, load balancing, API gateways, circuit breakers, and observability patterns."
        }
        1 => {
            "Describe the process of building a real-time collaborative text editor, covering conflict resolution strategies like CRDT and OT, network protocols, and cursor synchronization."
        }
        2 => {
            "Walk through the design of a distributed database, including consensus algorithms (Raft/Paxos), partitioning strategies, replication, and consistency models."
        }
        3 => {
            "Explain how a modern web browser renders a page, from DNS resolution through TCP/TLS, HTML parsing, CSSOM construction, layout, paint, and compositing."
        }
        _ => {
            "Describe the internals of a container runtime like Docker, covering namespaces, cgroups, union filesystems, image layers, and networking bridges."
        }
    };
    format!("Round {}: {}", round + 1, topic)
}

async fn run_execute_command(args: ExecuteArgs, config: &Config) -> Result<()> {
    let resolved = resolve_config(
        config,
        args.base_url.as_deref(),
        args.api_key.as_deref(),
        args.model.as_deref(),
    )?;

    println!("Executing Turn");
    println!("Prompt: {}", args.prompt);
    println!("Stream: {}", args.stream);
    println!();

    let provider = create_provider(&resolved)?;

    let mut thread = build_thread(provider, build_cli_agent_record("", vec![]), None)?;
    let record = thread
        .execute_turn(args.prompt, None, TurnCancellation::new())
        .await
        .map_err(|e| anyhow!("Turn failed: {}", e))?;
    let committed_messages: Vec<_> = thread.history_iter().cloned().collect();

    println!("Turn completed!");
    println!("Turn messages:");
    for (i, msg) in committed_messages.iter().enumerate() {
        let role_str = match msg.role {
            Role::User => "USER",
            Role::Assistant => "ASSISTANT",
            Role::System => "SYSTEM",
            Role::Tool => "TOOL",
        };
        let content = if msg.content.len() > 200 {
            format!("{}...", &msg.content[..200])
        } else {
            msg.content.clone()
        };
        println!("  [{}] {}: {}", i, role_str, content);
    }
    println!(
        "Tokens: {} input, {} output, {} total",
        record.token_usage.input_tokens,
        record.token_usage.output_tokens,
        record.token_usage.total_tokens
    );

    Ok(())
}

async fn tool_test(args: ToolTestArgs, config: &Config) -> Result<()> {
    let resolved = resolve_config(
        config,
        args.base_url.as_deref(),
        args.api_key.as_deref(),
        args.model.as_deref(),
    )?;

    println!("Testing Tool Execution");
    println!("Prompt: {}", args.prompt);
    println!();

    let provider = create_provider(&resolved)?;

    // System prompt that forces the assistant to use echo tool
    let system_prompt = r#"You are a helpful assistant that MUST use the echo tool to respond.

IMPORTANT RULES:
1. You MUST use the echo tool to echo back messages
2. After calling the echo tool and receiving the result, briefly mention what was echoed
3. Do NOT just respond with text - always use the echo tool first
4. The echo tool takes a "message" parameter with the string to echo

Example flow:
- User: "Echo hello"
- You: Call echo tool with {"message": "hello"}
- Tool returns: {"echoed": "hello"}
- You: "I echoed 'hello' for you""#;

    let tool_manager = Arc::new(ToolManager::new());
    tool_manager.register(Arc::new(EchoTool));
    let agent_record = build_cli_agent_record(system_prompt, vec!["echo"]);
    let mut thread = build_thread(provider, agent_record, Some(tool_manager))?;
    let record = thread
        .execute_turn(args.prompt, None, TurnCancellation::new())
        .await
        .map_err(|e| anyhow!("Turn failed: {}", e))?;
    let committed_messages: Vec<_> = thread.history_iter().cloned().collect();

    println!("Turn completed!");
    println!(
        "Total tool calls in conversation: {}",
        committed_messages
            .iter()
            .filter(|m| m.tool_calls.is_some())
            .count()
    );

    println!("Turn messages:");
    for (i, msg) in committed_messages.iter().enumerate() {
        let role_str = match msg.role {
            Role::User => "USER",
            Role::Assistant => "ASSISTANT",
            Role::System => "SYSTEM",
            Role::Tool => "TOOL",
        };
        let content = if msg.content.len() > 100 {
            format!("{}...", &msg.content[..100])
        } else {
            msg.content.clone()
        };
        println!("  [{}] {}: {}", i, role_str, content);

        if let Some(tool_calls) = &msg.tool_calls {
            for tc in tool_calls {
                println!(
                    "    Tool call: {} ({}) - args: {}",
                    tc.name, tc.id, tc.arguments
                );
            }
        }
    }

    println!(
        "Tokens: {} input, {} output, {} total",
        record.token_usage.input_tokens,
        record.token_usage.output_tokens,
        record.token_usage.total_tokens
    );

    Ok(())
}

async fn mock_test_turn(args: MockTestArgs) -> Result<()> {
    use argus_test_support::{AlwaysFailProvider, IntermittentFailureProvider};

    let provider: Arc<dyn LlmProvider> = match args.test_type.as_str() {
        "intermittent" => Arc::new(IntermittentFailureProvider::new()),
        "always-fail" => Arc::new(AlwaysFailProvider::new()),
        _ => return Err(anyhow!("Unknown test type: {}", args.test_type)),
    };

    let retry_provider = Arc::new(RetryProvider::new(
        provider,
        RetryConfig {
            max_retries: args.max_retries,
        },
    ));

    println!(
        "Testing {} with max_retries={}, stream={}",
        args.test_type, args.max_retries, args.stream
    );
    println!("Provider: {}", retry_provider.active_model_name());
    println!();
    if args.stream {
        // Test streaming turn execution
        todo!("Implement streaming test");
    } else {
        // Test simple turn execution
        let mut thread = build_thread(retry_provider, build_cli_agent_record("", vec![]), None)?;

        match thread
            .execute_turn("Test message".to_string(), None, TurnCancellation::new())
            .await
        {
            Ok(record) => {
                println!("Turn completed successfully!");
                println!(
                    "Tokens: {} input, {} output, {} total",
                    record.token_usage.input_tokens,
                    record.token_usage.output_tokens,
                    record.token_usage.total_tokens
                );
                Ok(())
            }
            Err(e) => {
                println!("Turn failed: {}", e);
                Err(anyhow!("Mock test failed: {}", e))
            }
        }
    }
}

async fn compact_test(args: CompactTestArgs, config: &Config) -> Result<()> {
    let resolved = resolve_config(
        config,
        args.base_url.as_deref(),
        args.api_key.as_deref(),
        args.model.as_deref(),
    )?;

    let provider = create_provider(&resolved)?;
    let context_window = provider.context_window();
    let threshold = (context_window as f32 * args.threshold_ratio) as u32;

    println!("=== Compaction Test ===");
    println!(
        "Provider: {} (context window: {} tokens)",
        resolved.model, context_window
    );
    println!(
        "Threshold: {} tokens ({:.0}% of context window)",
        threshold,
        args.threshold_ratio * 100.0
    );
    println!("Rounds: {}", args.rounds);
    println!();

    let mut thread = build_compact_thread(Arc::clone(&provider), args.threshold_ratio)?;
    let event_rx = thread.subscribe();
    let event_collector = collect_compaction_events(event_rx);

    for round in 0..args.rounds {
        let prompt = generate_round_prompt(round);
        println!("--- Round {} ---", round + 1);

        let _record = thread
            .execute_turn(prompt, None, TurnCancellation::new())
            .await
            .map_err(|e| anyhow!("Round {} failed: {}", round + 1, e))?;

        let history_count = thread.history_iter().count();
        let token_count = thread.token_count();

        println!(
            "  Tokens: {} total | History: {} messages",
            token_count, history_count
        );

        if token_count >= threshold {
            println!(
                "  Token count ({}) >= threshold ({}) - compaction should trigger on next turn",
                token_count, threshold
            );
        }

        // Give the event collector a moment to process events
        tokio::task::yield_now().await;
    }

    // Stop collecting events
    drop(thread);
    let compaction_events = event_collector
        .await
        .map_err(|e| anyhow!("Event collector failed: {}", e))?;
    let total_compactions = compaction_events.len();

    println!();
    println!("=== Results ===");
    println!("Total compaction events: {}", total_compactions);
    for event in &compaction_events {
        match event {
            ThreadEvent::CompactionStarted { thread_id } => {
                println!("  [STARTED] thread={}", thread_id);
            }
            ThreadEvent::Compacted {
                thread_id,
                new_token_count,
            } => {
                println!(
                    "  [COMPACTED] thread={}, new_token_count={}",
                    thread_id, new_token_count
                );
            }
            ThreadEvent::CompactionFinished { thread_id } => {
                println!("  [FINISHED] thread={}", thread_id);
            }
            ThreadEvent::CompactionFailed { thread_id, error } => {
                println!("  [FAILED] thread={}, error={}", thread_id, error);
            }
            _ => {}
        }
    }

    if total_compactions == 0 {
        println!();
        println!("No compaction triggered. Try:");
        println!("  - Increase --rounds (current: {})", args.rounds);
        println!(
            "  - Lower --threshold-ratio (current: {})",
            args.threshold_ratio
        );
        println!("  - Use a model with smaller context window");
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Load config
    let config = if let Some(path) = &cli.config {
        Config::load(path)
    } else {
        // Try default path
        let default_path = PathBuf::from("turn.toml");
        if default_path.exists() {
            Config::load(&default_path)
        } else {
            Config::default()
        }
    };

    match cli.command {
        Commands::Execute(args) => run_execute_command(args, &config).await?,
        Commands::ToolTest(args) => tool_test(args, &config).await?,
        Commands::MockTest(args) => mock_test_turn(args).await?,
        Commands::CompactTest(args) => compact_test(args, &config).await?,
    }

    Ok(())
}
