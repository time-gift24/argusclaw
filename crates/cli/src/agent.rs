//! Agent commands for interactive conversations.
//!
//! This is a production command, not behind the `dev` feature.

use std::io::{self, Write};
use std::sync::Arc;

use anyhow::{Result, anyhow};
use clap::Subcommand;

use claw::{AgentId, AgentRecord, AppContext, ApprovalDecision, ThreadConfig, ThreadEvent};
use claw::{GlobTool, GrepTool, LlmProviderId, ReadTool, ShellTool};
use tokio::io::AsyncBufReadExt;

use super::{StreamRenderState, finish_stream_output, render_stream_event};

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

        /// Tools that require approval (comma-separated, default: shell).
        #[arg(long, value_delimiter = ',', default_value = "shell")]
        approval_tools: Vec<String>,

        /// Tools to skip approval (comma-separated).
        #[arg(long, value_delimiter = ',')]
        muted_tools: Vec<String>,

        /// Auto-approve all tool calls (skip approval prompts).
        #[arg(long)]
        auto_approve: bool,
    },
}

/// Run an agent command.
pub async fn run_agent_command(ctx: AppContext, command: AgentCommand) -> Result<()> {
    match command {
        AgentCommand::Chat {
            provider,
            system_prompt,
            verbose,
            approval_tools,
            muted_tools,
            auto_approve,
        } => {
            run_chat(
                ctx,
                provider,
                system_prompt,
                verbose,
                approval_tools,
                muted_tools,
                auto_approve,
            )
            .await
        }
    }
}

async fn run_chat(
    ctx: AppContext,
    provider: Option<String>,
    system_prompt: String,
    verbose: bool,
    approval_tools: Vec<String>,
    muted_tools: Vec<String>,
    auto_approve: bool,
) -> Result<()> {
    // Resolve provider ID
    let provider_id = if let Some(id) = provider {
        // Verify provider exists
        ctx.get_provider(&LlmProviderId::new(&id))
            .await
            .map_err(|e| anyhow!("Failed to get provider '{}': {}", id, e))?;
        id
    } else {
        ctx.get_default_provider().await.map_err(|_| {
            anyhow!("No default provider configured. Use --provider or configure a default.")
        })?;
        ctx.get_default_provider_record()
            .await?
            .id
            .as_ref()
            .to_string()
    };

    // Filter out muted tools from approval list
    let effective_approval_tools: Vec<String> = approval_tools
        .into_iter()
        .filter(|t| !muted_tools.contains(t))
        .collect();

    // Register default tools
    let tool_manager = ctx.tool_manager();
    tool_manager.register(Arc::new(ShellTool::new()));
    tool_manager.register(Arc::new(ReadTool::new()));
    tool_manager.register(Arc::new(GrepTool::new()));
    tool_manager.register(Arc::new(GlobTool::new()));

    // Build AgentRecord
    let agent_record = AgentRecord {
        id: AgentId::new("cli-agent"),
        display_name: "CLI Agent".to_string(),
        description: "Interactive CLI agent".to_string(),
        version: "1.0.0".to_string(),
        provider_id,
        system_prompt,
        tool_names: vec![],
        max_tokens: None,
        temperature: None,
    };

    // Create agent via AppContext
    let agent_id = ctx
        .create_agent_with_approval(
            &agent_record,
            effective_approval_tools.clone(),
            auto_approve,
        )
        .await
        .map_err(|e| anyhow!("Failed to create agent: {}", e))?;

    // Create thread via AppContext
    let thread_id = ctx
        .create_thread(&agent_id, ThreadConfig::default())
        .map_err(|e| anyhow!("Failed to create thread: {}", e))?;

    // Subscribe to events via AppContext
    let mut event_rx = ctx
        .subscribe(&agent_id, &thread_id)
        .await
        .ok_or_else(|| anyhow!("Failed to subscribe to thread events"))?;

    // Print welcome message
    let has_approval_tools = !effective_approval_tools.is_empty();
    println!("{}", "─".repeat(50));
    println!("Interactive Agent Chat");
    println!("Type 'quit' or 'exit' to leave.");
    println!("Available tools: {}", tool_manager.list_ids().join(", "));
    if has_approval_tools {
        println!(
            "Approval required for: {}",
            effective_approval_tools.join(", ")
        );
        if auto_approve {
            println!("Auto-approve mode: ON");
        }
    }
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
                // Send message through AppContext
                ctx.send_message(&agent_id, &thread_id, input)
                    .await
                    .map_err(|e| anyhow!("Failed to send message: {}", e))?;

                // Stream rendering state
                let mut stream_state = StreamRenderState::default();
                print!("Assistant: ");
                io::stdout().flush()?;

                // Process events until Idle
                loop {
                    match event_rx.recv().await {
                        Ok(ThreadEvent::Processing { event, .. }) => {
                            if let Some(output) = render_stream_event(&mut stream_state, &event) {
                                print!("{}", output);
                                io::stdout().flush()?;
                            }
                        }
                        Ok(ThreadEvent::ToolStarted {
                            tool_name,
                            tool_call_id,
                            arguments,
                            ..
                        }) => {
                            eprintln!("\n🔧 Tool: {} ({})", tool_name, tool_call_id);
                            if verbose {
                                eprintln!("   Args: {:?}", arguments);
                            }
                        }
                        Ok(ThreadEvent::ToolCompleted { result, .. }) => match result {
                            Ok(value) => {
                                let result_str = format!("{:?}", value);
                                if verbose {
                                    eprintln!("   ✅ Result: {}", result_str);
                                } else {
                                    let truncated = if result_str.len() > 100 {
                                        format!("{}...", &result_str[..100])
                                    } else {
                                        result_str
                                    };
                                    eprintln!("   ✅ {}", truncated);
                                }
                            }
                            Err(e) => {
                                eprintln!("   ❌ Error: {}", e);
                            }
                        },
                        Ok(ThreadEvent::TurnCompleted { token_usage, .. }) => {
                            if let Some(suffix) = finish_stream_output(&stream_state) {
                                print!("{}", suffix);
                            }

                            if verbose {
                                eprintln!(
                                    "  [tokens: {} in / {} out]",
                                    token_usage.input_tokens, token_usage.output_tokens
                                );
                            }
                        }
                        Ok(ThreadEvent::TurnFailed { error, .. }) => {
                            eprintln!("Error: {}", error);
                        }
                        Ok(ThreadEvent::Idle { .. }) => {
                            break;
                        }
                        Ok(ThreadEvent::Compacted {
                            new_token_count, ..
                        }) => {
                            if verbose {
                                eprintln!("  [compacted: new token count = {}]", new_token_count);
                            }
                        }
                        Ok(ThreadEvent::WaitingForApproval { request, .. }) => {
                            eprintln!(
                                "\n⏳ Waiting for approval: {} ({})",
                                request.tool_name, request.action
                            );
                            eprintln!("   Risk level: {:?}", request.risk_level);
                            eprintln!("   Timeout: {}s", request.timeout_secs);

                            if auto_approve {
                                eprintln!("   ⚡ Auto-approving...");
                                let request_id = request.id;
                                let ctx_clone = ctx.clone();
                                let agent_id_clone = agent_id.clone();
                                tokio::spawn(async move {
                                    let _ =
                                        tokio::time::sleep(std::time::Duration::from_millis(100))
                                            .await;
                                    let _ = ctx_clone.resolve_approval(
                                        &agent_id_clone,
                                        request_id,
                                        ApprovalDecision::Approved,
                                        Some("auto-approve".to_string()),
                                    );
                                });
                            } else {
                                eprint!("   Approve? [y/n/timeout] (default: y): ");
                                io::stderr().flush()?;

                                let mut response = String::new();
                                match io::stdin().read_line(&mut response) {
                                    Ok(_) => {
                                        let response = response.trim().to_lowercase();
                                        let decision = match response.as_str() {
                                            "n" | "no" | "deny" => ApprovalDecision::Denied,
                                            "t" | "timeout" => ApprovalDecision::TimedOut,
                                            _ => ApprovalDecision::Approved,
                                        };

                                        let _ = ctx.resolve_approval(
                                            &agent_id,
                                            request.id,
                                            decision,
                                            Some("cli-user".to_string()),
                                        );
                                    }
                                    Err(e) => {
                                        eprintln!("   Failed to read input: {}. Denying...", e);
                                        let _ = ctx.resolve_approval(
                                            &agent_id,
                                            request.id,
                                            ApprovalDecision::Denied,
                                            Some("cli-error".to_string()),
                                        );
                                    }
                                }
                            }
                        }
                        Ok(ThreadEvent::ApprovalResolved { response, .. }) => {
                            match response.decision {
                                ApprovalDecision::Approved => {
                                    eprintln!("✅ Approval: APPROVED");
                                }
                                ApprovalDecision::Denied => {
                                    eprintln!("❌ Approval: DENIED");
                                }
                                ApprovalDecision::TimedOut => {
                                    eprintln!("⏰ Approval: TIMED OUT");
                                }
                            }
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                            eprintln!("Channel closed unexpectedly");
                            break;
                        }
                        Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                            if verbose {
                                eprintln!("  [warning: {} events lagged]", n);
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
