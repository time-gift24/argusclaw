//! argus-llm CLI - Test LLM connections and retry capabilities.
//!
//! Usage:
//!   argus-llm test --base-url URL --api-key KEY --model MODEL
//!   argus-llm complete --prompt "Hello" [--stream]
//!   argus-llm retry-test --max-retries 5

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Result, anyhow};
use clap::{Args, Parser, Subcommand};
use serde::Deserialize;

use argus_llm::providers::{
    OpenAiCompatibleConfig, OpenAiCompatibleFactoryConfig, create_openai_compatible_provider,
};
use argus_llm::retry::{RetryConfig, RetryProvider};
use argus_protocol::llm::{ChatMessage, CompletionRequest, LlmProvider, LlmStreamEvent};

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

/// Common arguments for all commands.
trait CommandArgs {
    fn base_url(&self) -> &Option<String>;
    fn api_key(&self) -> &Option<String>;
    fn model(&self) -> &Option<String>;
}

#[derive(Parser)]
#[command(
    name = "argus-llm",
    version,
    about = "Test LLM connections and retry capabilities"
)]
struct Cli {
    /// Configuration file path (default: ./llm.toml)
    #[arg(short, long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Test LLM connection.
    Test(TestArgs),
    /// Complete a prompt.
    Complete(CompleteArgs),
    /// Test retry capability.
    RetryTest(RetryTestArgs),
    /// Test retry behavior with mock providers.
    MockTest(MockTestArgs),
}

#[derive(Args)]
struct TestArgs {
    /// API base URL
    #[arg(long, env = "ARGUS_LLM_BASE_URL")]
    base_url: Option<String>,
    /// API key
    #[arg(long, env = "ARGUS_LLM_API_KEY")]
    api_key: Option<String>,
    /// Model name
    #[arg(long, env = "ARGUS_LLM_MODEL")]
    model: Option<String>,
}

impl CommandArgs for TestArgs {
    fn base_url(&self) -> &Option<String> {
        &self.base_url
    }
    fn api_key(&self) -> &Option<String> {
        &self.api_key
    }
    fn model(&self) -> &Option<String> {
        &self.model
    }
}

#[derive(Args)]
struct CompleteArgs {
    /// API base URL
    #[arg(long, env = "ARGUS_LLM_BASE_URL")]
    base_url: Option<String>,
    /// API key
    #[arg(long, env = "ARGUS_LLM_API_KEY")]
    api_key: Option<String>,
    /// Model name
    #[arg(long, env = "ARGUS_LLM_MODEL")]
    model: Option<String>,
    /// The prompt to complete
    #[arg(long)]
    prompt: String,
    /// Stream the response
    #[arg(long, default_value_t = false)]
    stream: bool,
    /// Test retry behavior by injecting intermittent failures
    #[arg(long, default_value_t = false)]
    test_retry: bool,
    /// Maximum retries for test mode
    #[arg(long, default_value_t = 3)]
    max_retries: u32,
}

impl CommandArgs for CompleteArgs {
    fn base_url(&self) -> &Option<String> {
        &self.base_url
    }
    fn api_key(&self) -> &Option<String> {
        &self.api_key
    }
    fn model(&self) -> &Option<String> {
        &self.model
    }
}

#[derive(Args)]
struct RetryTestArgs {
    /// API base URL
    #[arg(long, env = "ARGUS_LLM_BASE_URL")]
    base_url: Option<String>,
    /// API key
    #[arg(long, env = "ARGUS_LLM_API_KEY")]
    api_key: Option<String>,
    /// Model name
    #[arg(long, env = "ARGUS_LLM_MODEL")]
    model: Option<String>,
    /// Maximum number of retries
    #[arg(long, default_value_t = 3)]
    max_retries: u32,
}

impl CommandArgs for RetryTestArgs {
    fn base_url(&self) -> &Option<String> {
        &self.base_url
    }
    fn api_key(&self) -> &Option<String> {
        &self.api_key
    }
    fn model(&self) -> &Option<String> {
        &self.model
    }
}

#[derive(Args)]
struct MockTestArgs {
    /// Test type: "intermittent" or "always-fail"
    #[arg(long)]
    test_type: String,
    /// Maximum number of retries
    #[arg(long, default_value_t = 3)]
    max_retries: u32,
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

fn create_retry_provider(provider: Arc<dyn LlmProvider>, max_retries: u32) -> Arc<dyn LlmProvider> {
    Arc::new(RetryProvider::new(provider, RetryConfig { max_retries }))
}

fn resolve_config(config: &Config, command_args: &dyn CommandArgs) -> ResolvedConfig {
    ResolvedConfig {
        base_url: command_args
            .base_url()
            .clone()
            .or_else(|| std::env::var("ARGUS_LLM_BASE_URL").ok())
            .or_else(|| config.base_url.clone())
            .unwrap_or_else(|| "https://api.openai.com/v1".to_string()),
        api_key: command_args
            .api_key()
            .clone()
            .or_else(|| std::env::var("ARGUS_LLM_API_KEY").ok())
            .or_else(|| config.api_key.clone())
            .expect("API key is required (set --api-key, ARGUS_LLM_API_KEY, or config file)"),
        model: command_args
            .model()
            .clone()
            .or_else(|| std::env::var("ARGUS_LLM_MODEL").ok())
            .or_else(|| config.model.clone())
            .unwrap_or_else(|| "gpt-4o-mini".to_string()),
    }
}

async fn test_connection(config: &ResolvedConfig) -> Result<()> {
    let provider = create_provider(config)?;

    println!("Testing connection to {}...", config.base_url);
    println!("Model: {}", config.model);

    let started = Instant::now();
    let request = CompletionRequest::new(vec![ChatMessage::user("Reply with exactly OK.")])
        .with_max_tokens(8)
        .with_temperature(0.0);

    match provider.complete(request).await {
        Ok(response) => {
            let latency = started.elapsed();
            println!("✓ Connection successful ({}ms)", latency.as_millis());
            println!("Response: {}", response.content.as_deref().unwrap_or("").trim());
            Ok(())
        }
        Err(e) => {
            println!("✗ Connection failed: {}", e);
            Err(anyhow!("Connection test failed: {}", e))
        }
    }
}

async fn complete_prompt(
    config: &ResolvedConfig,
    prompt: &str,
    stream: bool,
    test_retry: bool,
    max_retries: u32,
) -> Result<()> {
    let base_provider = create_provider(config)?;

    // Wrap provider with test retry behavior if requested
    let provider = if test_retry {
        println!("🧪 Test mode: Injecting intermittent failures");
        println!("📊 Max retries: {}", max_retries);
        println!();
        argus_llm::create_test_retry_provider(base_provider, max_retries)
    } else {
        base_provider
    };

    println!("Completing prompt: \"{}\"", prompt);
    println!("Model: {}", config.model);
    println!();

    let request = CompletionRequest::new(vec![ChatMessage::user(prompt.to_string())]);

    if stream {
        let event_stream = provider
            .stream_complete(request)
            .await
            .map_err(|e| anyhow!("Failed to start stream: {}", e))?;

        let mut stream = event_stream;
        use futures_util::StreamExt;
        use std::io::Write;

        // Track state for printing section headers and retry events
        let mut reasoning_started = false;
        let mut summary_started = false;
        let mut retry_count = 0;

        while let Some(event) = stream.next().await {
            match event {
                Ok(LlmStreamEvent::RetryAttempt {
                    attempt,
                    max_retries,
                    error,
                }) => {
                    retry_count += 1;
                    println!("🔄 Retry attempt {}/{}: {}", attempt, max_retries, error);
                }
                Ok(LlmStreamEvent::ReasoningDelta { delta }) if !delta.is_empty() => {
                    if !reasoning_started {
                        print!("[Reasoning] ");
                        reasoning_started = true;
                    }
                    print!("{}", delta);
                    std::io::stdout().flush().ok();
                }
                Ok(LlmStreamEvent::ContentDelta { delta }) if !delta.is_empty() => {
                    // Transition from reasoning to summary: add newline before summary
                    if reasoning_started && !summary_started {
                        println!();
                        print!("[Summary] ");
                        summary_started = true;
                    }
                    if !summary_started {
                        print!("[Summary] ");
                        summary_started = true;
                    }
                    print!("{}", delta);
                    std::io::stdout().flush().ok();
                }
                Ok(LlmStreamEvent::Finished { .. }) => {
                    println!();
                }
                Err(e) => {
                    eprintln!("\nError: {}", e);
                    return Err(anyhow!("Stream error: {}", e));
                }
                _ => {}
            }
        }

        if test_retry {
            println!();
            if retry_count > 0 {
                println!("📊 Total retries: {}", retry_count);
            } else {
                println!("📊 No retries occurred");
            }
        }
    } else {
        let started = Instant::now();
        let response = provider
            .complete(request)
            .await
            .map_err(|e| anyhow!("Completion failed: {}", e))?;

        println!("{}", response.content.as_deref().unwrap_or(""));
        println!();
        println!(
            "Tokens: {} input, {} output ({}ms)",
            response.input_tokens,
            response.output_tokens,
            started.elapsed().as_millis()
        );

        if test_retry {
            println!();
            println!("💡 Tip: Use --stream to see retry events in real-time");
            println!(
                "   Example: cargo run --bin argus-llm -- complete --prompt '{}' --stream --test-retry",
                prompt
            );
        }
    }

    Ok(())
}

async fn test_retry(config: &ResolvedConfig, max_retries: u32) -> Result<()> {
    let base_provider = create_provider(config)?;
    let provider = create_retry_provider(base_provider, max_retries);

    println!("Testing retry capability...");
    println!("Model: {}", config.model);
    println!("Max retries: {}", max_retries);
    println!();

    let request = CompletionRequest::new(vec![ChatMessage::user(
        "Reply with exactly the word 'pong'.",
    )])
    .with_max_tokens(8)
    .with_temperature(0.0);

    let started = Instant::now();
    match provider.complete(request).await {
        Ok(response) => {
            println!("✓ Request completed successfully");
            println!("Response: {}", response.content.as_deref().unwrap_or("").trim());
            println!("Total time: {}ms", started.elapsed().as_millis());
            Ok(())
        }
        Err(e) => {
            println!("✗ Request failed after {} retries: {}", max_retries, e);
            Err(anyhow!("Retry test failed: {}", e))
        }
    }
}

async fn mock_test(test_type: &str, max_retries: u32) -> Result<()> {
    use argus_test_support::{AlwaysFailProvider, IntermittentFailureProvider};

    let provider: Arc<dyn LlmProvider> = match test_type {
        "intermittent" => Arc::new(IntermittentFailureProvider::new()),
        "always-fail" => Arc::new(AlwaysFailProvider::new()),
        _ => return Err(anyhow!("Unknown test type: {}", test_type)),
    };

    let retry_provider = create_retry_provider(provider, max_retries);

    println!("Testing {} with max_retries={}", test_type, max_retries);
    println!("Provider: {}", retry_provider.active_model_name());
    println!();

    // Test streaming call to capture retry events
    let request = CompletionRequest::new(vec![ChatMessage::user("Test message".to_string())]);

    let event_stream = retry_provider.stream_complete(request).await?;

    // Process events
    use futures_util::StreamExt;
    let mut stream = event_stream;
    let mut retry_count = 0;
    let mut had_error = false;

    while let Some(event) = stream.next().await {
        match event {
            Ok(LlmStreamEvent::RetryAttempt {
                attempt,
                max_retries,
                error,
            }) => {
                retry_count += 1;
                println!("🔄 Retry attempt {}/{}: {}", attempt, max_retries, error);
            }
            Ok(LlmStreamEvent::Finished { finish_reason }) => {
                println!("✓ Stream finished: {:?}", finish_reason);
            }
            Err(e) => {
                println!("✗ Stream error: {}", e);
                had_error = true;
            }
            _ => {}
        }
    }

    println!();
    if retry_count > 0 {
        println!("📊 Total retries: {}", retry_count);
    } else {
        println!("📊 No retries occurred (request succeeded on first attempt)");
    }

    if had_error {
        Err(anyhow!("Mock test failed after {} retries", retry_count))
    } else {
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    // Determine config file path
    let config_path = cli
        .config
        .clone()
        .unwrap_or_else(|| PathBuf::from("llm.toml"));

    // Load config file
    let config = Config::load(&config_path);

    // Handle commands
    match &cli.command {
        Commands::Test(args) => {
            let resolved = resolve_config(&config, args);
            test_connection(&resolved).await
        }
        Commands::Complete(args) => {
            let resolved = resolve_config(&config, args);
            complete_prompt(
                &resolved,
                &args.prompt,
                args.stream,
                args.test_retry,
                args.max_retries,
            )
            .await
        }
        Commands::RetryTest(args) => {
            let resolved = resolve_config(&config, args);
            test_retry(&resolved, args.max_retries).await
        }
        Commands::MockTest(args) => mock_test(&args.test_type, args.max_retries).await,
    }
}
