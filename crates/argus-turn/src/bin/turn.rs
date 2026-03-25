//! argus-turn CLI - Test Turn execution with tools and hooks.
//!
//! Usage:
//!   argus-turn execute --prompt "Hello" [--stream]
//!   argus-turn tool-test --prompt "What is 2+2?"

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Result, anyhow};
use async_trait::async_trait;
use clap::{Args, Parser, Subcommand};
use serde::Deserialize;
use tokio::sync::broadcast;

use argus_llm::providers::{
    OpenAiCompatibleConfig, OpenAiCompatibleFactoryConfig, create_openai_compatible_provider,
};
use argus_llm::retry::{RetryConfig, RetryProvider};
use argus_protocol::llm::{ChatMessage, LlmProvider, Role, ToolDefinition};
use argus_protocol::tool::{NamedTool, ToolError, ToolExecutionContext};
use argus_turn::{TurnBuilder, TurnConfig, TurnStreamEvent};

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

    async fn execute(&self, args: serde_json::Value, _ctx: Arc<ToolExecutionContext>) -> Result<serde_json::Value, ToolError> {
        let message = args
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

async fn execute_turn(args: ExecuteArgs, config: &Config) -> Result<()> {
    let resolved = resolve_config(
        config,
        args.base_url.as_deref(),
        args.api_key.as_deref(),
        args.model.as_deref(),
    )?;

    println!("🚀 Executing Turn");
    println!("📝 Prompt: {}", args.prompt);
    println!("🔧 Stream: {}", args.stream);
    println!();

    let provider = create_provider(&resolved)?;

    // Create channels
    let (stream_tx, _) = broadcast::channel::<TurnStreamEvent>(256);
    let (thread_event_tx, _) = broadcast::channel(256);

    // Build turn
    let turn = TurnBuilder::default()
        .turn_number(1)
        .thread_id("test-thread".to_string())
        .messages(vec![ChatMessage::user(&args.prompt)])
        .provider(provider)
        .tools(vec![])
        .hooks(vec![])
        .config(TurnConfig::default())
        .stream_tx(stream_tx)
        .thread_event_tx(thread_event_tx)
        .build()
        .map_err(|e| anyhow!("Failed to build turn: {}", e))?;

    // Execute turn (no streaming for now due to Send trait issue)
    let output = turn.execute().await?;

    println!("✅ Turn completed!");
    println!("📊 Messages:");
    for (i, msg) in output.messages.iter().enumerate() {
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
        "📊 Tokens: {} input, {} output, {} total",
        output.token_usage.input_tokens,
        output.token_usage.output_tokens,
        output.token_usage.total_tokens
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

    println!("🧪 Testing Tool Execution");
    println!("📝 Prompt: {}", args.prompt);
    println!();

    let provider = create_provider(&resolved)?;

    // Create channels
    let (stream_tx, _) = broadcast::channel::<TurnStreamEvent>(256);
    let (thread_event_tx, _) = broadcast::channel(256);

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

    // Build turn with echo tool and system prompt
    let turn = TurnBuilder::default()
        .turn_number(1)
        .thread_id("test-thread".to_string())
        .messages(vec![
            ChatMessage::system(system_prompt),
            ChatMessage::user(&args.prompt),
        ])
        .provider(provider)
        .tools(vec![Arc::new(EchoTool) as Arc<dyn NamedTool>])
        .hooks(vec![])
        .config(TurnConfig::default())
        .stream_tx(stream_tx)
        .thread_event_tx(thread_event_tx)
        .build()
        .map_err(|e| anyhow!("Failed to build turn: {}", e))?;

    // Execute turn
    let output = turn.execute().await?;

    println!("✅ Turn completed!");
    println!(
        "📊 Total tool calls in conversation: {}",
        output
            .messages
            .iter()
            .filter(|m| m.tool_calls.is_some())
            .count()
    );

    println!("📊 Messages:");
    for (i, msg) in output.messages.iter().enumerate() {
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
                    "    🔧 Tool call: {} ({}) - args: {}",
                    tc.name, tc.id, tc.arguments
                );
            }
        }
    }

    println!(
        "📊 Tokens: {} input, {} output, {} total",
        output.token_usage.input_tokens,
        output.token_usage.output_tokens,
        output.token_usage.total_tokens
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

    let messages = vec![ChatMessage::user("Test message".to_string())];

    if args.stream {
        // Test streaming turn execution
        todo!("Implement streaming test");
    } else {
        // Test simple turn execution
        let (stream_tx, _stream_rx) = broadcast::channel::<TurnStreamEvent>(256);
        let (thread_event_tx, _thread_event_rx) = broadcast::channel(256);

        let turn = TurnBuilder::default()
            .turn_number(1)
            .thread_id("test-thread".to_string())
            .messages(messages)
            .provider(retry_provider)
            .tools(vec![])
            .hooks(vec![])
            .config(TurnConfig::default())
            .stream_tx(stream_tx)
            .thread_event_tx(thread_event_tx)
            .build()
            .map_err(|e| anyhow!("Failed to build turn: {}", e))?;

        match turn.execute().await {
            Ok(output) => {
                println!("✓ Turn completed successfully!");
                println!(
                    "📊 Tokens: {} input, {} output, {} total",
                    output.token_usage.input_tokens,
                    output.token_usage.output_tokens,
                    output.token_usage.total_tokens
                );
                Ok(())
            }
            Err(e) => {
                println!("✗ Turn failed: {}", e);
                Err(anyhow!("Mock test failed: {}", e))
            }
        }
    }
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
        Commands::Execute(args) => execute_turn(args, &config).await?,
        Commands::ToolTest(args) => tool_test(args, &config).await?,
        Commands::MockTest(args) => mock_test_turn(args).await?,
    }

    Ok(())
}
