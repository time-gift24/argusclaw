//! Agent commands for interactive conversations.
//!
//! This is a production command, not behind the `dev` feature.

use std::io::{self, Write};

use anyhow::{Result, anyhow};
use clap::Subcommand;

use claw::AppContext;
use claw::agents::compact::KeepRecentCompactor;
use claw::agents::thread::{ThreadBuilder, ThreadConfig, ThreadEvent};
use claw::db::llm::LlmProviderId;
use claw::llm::{ChatMessage, Role};
use tokio::io::AsyncBufReadExt;

/// Agent subcommands.
#[derive(Debug, Subcommand)]
pub enum AgentCommand {
    /// Start interactive conversation with a default agent.
    Chat {
        /// Provider ID to use (defaults to default provider).
        #[arg(long)]
        provider: Option<String>,

        /// System prompt for the agent.
        #[arg(long, default_value = "You are a helpful assistant.")]
        system_prompt: String,

        /// Enable verbose output (shows token usage).
        #[arg(short, long)]
        verbose: bool,
    },
}

/// Run an agent command.
pub async fn run_agent_command(ctx: AppContext, command: AgentCommand) -> Result<()> {
    match command {
        AgentCommand::Chat {
            provider,
            system_prompt,
            verbose,
        } => run_chat(ctx, provider, system_prompt, verbose).await,
    }
}

async fn run_chat(
    ctx: AppContext,
    provider: Option<String>,
    system_prompt: String,
    verbose: bool,
) -> Result<()> {
    // Get provider
    let llm_provider = if let Some(id) = provider {
        ctx.llm_manager()
            .get_provider(&LlmProviderId::new(&id))
            .await
            .map_err(|e| anyhow!("Failed to get provider '{}': {}", id, e))?
    } else {
        ctx.llm_manager()
            .get_default_provider()
            .await
            .map_err(|_| {
                anyhow!("No default provider configured. Use --provider or configure a default.")
            })?
    };

    // Create Thread
    let compactor = std::sync::Arc::new(KeepRecentCompactor::with_defaults());
    let mut thread = ThreadBuilder::new()
        .provider(llm_provider)
        .tool_manager(ctx.tool_manager())
        .compactor(compactor)
        .config(ThreadConfig::default())
        .build();

    // Add system prompt
    thread
        .messages_mut()
        .push(ChatMessage::system(&system_prompt));

    // Subscribe to events
    let mut event_rx = thread.subscribe();

    // Print welcome message
    println!("{}", "─".repeat(50));
    println!("Interactive Agent Chat");
    println!("Type 'quit' or 'exit' to leave.");
    println!("{}", "─".repeat(50));

    // Interactive loop
    let stdin = tokio::io::BufReader::new(tokio::io::stdin());
    let mut lines = stdin.lines();

    loop {
        print!("You: ");
        io::stdout().flush()?;

        let line = lines.next_line().await?;
        match line {
            Some(input) if input == "quit" || input == "exit" => {
                println!("Goodbye!");
                break;
            }
            Some(input) if !input.is_empty() => {
                // Send message
                thread.send_message(input).await;

                // Process events until Idle
                loop {
                    match event_rx.recv().await {
                        Ok(ThreadEvent::Processing { event, .. }) => {
                            if verbose {
                                println!("  [stream: {:?}]", event);
                            }
                        }
                        Ok(ThreadEvent::TurnCompleted { token_usage, .. }) => {
                            // Find and print the last assistant message
                            if let Some(content) = thread
                                .messages
                                .iter()
                                .rev()
                                .find(|m| m.role == Role::Assistant)
                                .map(|m| m.content.clone())
                            {
                                println!("Assistant: {}", content);
                            }

                            if verbose {
                                println!(
                                    "  [tokens: {} in / {} out]",
                                    token_usage.input_tokens, token_usage.output_tokens
                                );
                            }
                        }
                        Ok(ThreadEvent::TurnFailed { error, .. }) => {
                            println!("Error: {}", error);
                        }
                        Ok(ThreadEvent::Idle { .. }) => {
                            // Turn finished, ready for next input
                            break;
                        }
                        Ok(ThreadEvent::Compacted {
                            new_token_count, ..
                        }) => {
                            if verbose {
                                println!("  [compacted: new token count = {}]", new_token_count);
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            println!("Channel closed unexpectedly");
                            break;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            if verbose {
                                println!("  [warning: {} events lagged]", n);
                            }
                        }
                    }
                }
            }
            None => break,
            _ => continue,
        }
    }

    Ok(())
}
