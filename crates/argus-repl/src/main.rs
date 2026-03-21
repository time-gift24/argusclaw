//! argus-repl: Interactive REPL for end-to-end testing.

use std::sync::Arc;

use argus_protocol::{ThreadEvent, ThreadId};
use argus_repl::mock_provider::ReplMockProvider;
use argus_thread::{KeepRecentCompactor, ThreadBuilder};
use argus_tool::{GlobTool, GrepTool, HttpTool, ReadTool, ToolManager};
use argus_wing::ArgusWing;
use clap::Parser;
use tokio::{
    io::AsyncBufReadExt,
    signal::ctrl_c,
    sync::Mutex,
};

/// Parse command-line arguments.
#[derive(Debug, Parser)]
#[command(name = "argus-repl")]
#[command(about = "Interactive REPL for end-to-end ArgusClaw testing")]
struct Args {
    /// Enable verbose output (shows token stats and detailed events).
    #[arg(long)]
    verbose: bool,

    /// Custom database path.
    #[arg(long)]
    db: Option<String>,

    /// Enable debug logging.
    #[arg(long, default_value = "false")]
    debug: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    // Initialize tracing
    let filter = if args.debug {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("debug"))
    } else {
        tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"))
    };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();

    // Initialize ArgusWing
    let wing = ArgusWing::init(args.db.as_deref())
        .await
        .expect("ArgusWing::init failed");

    // Create tool manager with safe tools only (no ShellTool)
    let tool_manager = Arc::new({
        let tm = ToolManager::new();
        tm.register(Arc::new(ReadTool::new()));
        tm.register(Arc::new(GrepTool::new()));
        tm.register(Arc::new(GlobTool::new()));
        tm.register(Arc::new(HttpTool::new()));
        tm
    });

    // Get default template (for agent record)
    let template = wing
        .get_default_template()
        .await?
        .expect("Default template 'ArgusWing' not found");

    // Create session
    let session_id = wing
        .create_session("repl-session")
        .await
        .expect("Failed to create session");

    // Create mock provider
    let mock_provider: Arc<dyn argus_protocol::llm::LlmProvider> =
        Arc::new(ReplMockProvider::new());

    // Get compactor
    let compactor: Arc<dyn argus_thread::compact::Compactor> =
        Arc::new(KeepRecentCompactor::with_defaults());

    // Build thread with ThreadBuilder directly (bypassing ProviderResolver)
    let thread = ThreadBuilder::new()
        .id(ThreadId::new())
        .session_id(session_id)
        .agent_record(Arc::new(template))
        .provider(mock_provider)
        .tool_manager(tool_manager)
        .compactor(compactor)
        .config(argus_thread::ThreadConfig::default())
        .build()
        .expect("ThreadBuilder::build failed");

    let thread = Arc::new(Mutex::new(thread));
    let thread_id = thread.lock().await.id();

    println!(
        "[Argus REPL] Session #{} created, thread: {}",
        session_id.inner(),
        thread_id
    );

    // Subscribe to thread events
    let mut event_rx = {
        let t = thread.lock().await;
        t.subscribe()
    };

    // Flag to exit REPL
    let should_exit = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let should_exit_clone = should_exit.clone();

    // Spawn event consumer task
    let event_handle = tokio::spawn(async move {
        let mut content_buffer = String::new();
        while let Ok(event) = event_rx.recv().await {
            match &event {
                ThreadEvent::Processing { event: llm_event, .. } => {
                    match llm_event {
                        argus_protocol::llm::LlmStreamEvent::ReasoningDelta { delta } => {
                            println!("[Reasoning] {}", delta);
                        }
                        argus_protocol::llm::LlmStreamEvent::ContentDelta { delta } => {
                            content_buffer.push_str(delta);
                        }
                        argus_protocol::llm::LlmStreamEvent::ToolCallDelta { .. } => {
                            // Mock never emits this, but handle it anyway
                        }
                        argus_protocol::llm::LlmStreamEvent::Usage { .. } => {
                            // Usage events ignored in default mode
                        }
                        argus_protocol::llm::LlmStreamEvent::Finished { .. } => {
                            if !content_buffer.is_empty() {
                                println!("[Content] {}", content_buffer);
                                content_buffer.clear();
                            }
                        }
                        argus_protocol::llm::LlmStreamEvent::RetryAttempt {
                            attempt,
                            max_retries,
                            error,
                        } => {
                            if args.verbose {
                                println!("[Retry] {}/{}: {}", attempt, max_retries, error);
                            }
                        }
                    }
                }
                ThreadEvent::ToolStarted { tool_name, .. } => {
                    if args.verbose {
                        println!("[ToolStarted] {}", tool_name);
                    }
                }
                ThreadEvent::ToolCompleted { tool_name, result, .. } => {
                    let display = match result {
                        Ok(value) => {
                            let s = serde_json::to_string(value).unwrap_or_else(|_| value.to_string());
                            if s.len() > 200 {
                                format!("{}...", &s[..200])
                            } else {
                                s
                            }
                        }
                        Err(e) => format!("ERROR: {}", e),
                    };
                    println!("[ToolCompleted] {}: {}", tool_name, display);
                }
                ThreadEvent::TurnCompleted { token_usage, .. } => {
                    if args.verbose {
                        println!(
                            "[Event: TurnCompleted] tokens={} input, {} output",
                            token_usage.input_tokens, token_usage.output_tokens
                        );
                    }
                }
                ThreadEvent::TurnFailed { error, .. } => {
                    println!("[Error] {}", error);
                }
                ThreadEvent::WaitingForApproval { request, .. } => {
                    println!("[Approval] {} pending", request.tool_name);
                }
                _ => {
                    // ApprovalResolved, Idle, Compacted — ignored
                }
            }
        }
    });

    // REPL input loop
    let stdin = tokio::io::stdin();
    let mut lines = tokio::io::BufReader::new(stdin).lines();

    // Also listen for Ctrl+C
    tokio::spawn(async move {
        ctrl_c().await.ok();
        should_exit_clone.store(true, std::sync::atomic::Ordering::SeqCst);
    });

    loop {
        if should_exit.load(std::sync::atomic::Ordering::SeqCst) {
            println!("\n[Argus REPL] Goodbye.");
            break;
        }

        tokio::io::AsyncWriteExt::write_all(&mut tokio::io::stdout(), b"> ")
            .await
            .ok();
        tokio::io::AsyncWriteExt::flush(&mut tokio::io::stdout())
            .await
            .ok();

        let line = lines.next_line().await;
        let line = match line {
            Ok(Some(l)) => l,
            Ok(None) => break,
            Err(_) => break,
        };

        let input = line.trim();
        if input.is_empty() || input == "exit" {
            println!("[Argus REPL] Goodbye.");
            break;
        }

        // Send message
        let mut t = thread.lock().await;
        if let Err(e) = t.send_message(input.to_string(), None).await {
            println!("[Error] send_message failed: {}", e);
        }
    }

    // Wait for event consumer
    event_handle.abort();

    Ok(())
}
