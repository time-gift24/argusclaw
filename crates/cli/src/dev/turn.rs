//! Turn command - development only.

use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use clap::Subcommand;
use claw::turn::{TurnConfig, TurnInputBuilder, execute_turn};
use claw::{AppContext, ChatMessage, LlmProviderId};
use owo_colors::OwoColorize;

/// Turn 执行测试命令。
#[derive(Debug, Subcommand)]
pub enum TurnCommand {
    /// 测试 Turn 执行（可配置选项）。
    Test {
        /// 使用的提供商 ID（默认为默认提供商）。
        #[arg(long)]
        provider: Option<String>,
        /// 启用的工具 ID（逗号分隔）。
        #[arg(long, value_delimiter = ',')]
        tools: Vec<String>,
        /// Turn 的系统提示词。
        #[arg(long, default_value = "You are a helpful assistant.")]
        system_prompt: String,
        /// 用户消息。
        #[arg(default_value = "Hello")]
        message: String,
        /// 从文件读取消息。
        #[arg(long)]
        input: Option<String>,
        /// 启用详细输出（显示所有消息和工具调用）。
        #[arg(short, long)]
        verbose: bool,
    },
}

/// Run turn command.
pub async fn run_turn_command(ctx: AppContext, command: TurnCommand) -> Result<()> {
    let TurnCommand::Test {
        provider,
        tools,
        system_prompt,
        message,
        input,
        verbose,
    } = command;

    // Read message from file or use direct message
    let message = if let Some(path) = input {
        std::fs::read_to_string(&path)
            .with_context(|| format!("Failed to read input file: {}", path))?
    } else {
        message
    };

    // Get provider from context via LLM manager
    let provider = if let Some(id) = provider {
        ctx.get_provider(&LlmProviderId::new(id)).await?
    } else {
        ctx.get_default_provider().await?
    };

    // Build tool manager with requested tools
    let tool_manager = Arc::new(claw::ToolManager::new());
    // TODO: Register tools based on IDs when tools are implemented

    // Build turn input
    let input = TurnInputBuilder::default()
        .provider(provider)
        .messages(vec![ChatMessage::user(message)])
        .system_prompt(system_prompt)
        .tool_manager(tool_manager)
        .tool_ids(tools)
        .build()
        .context("Failed to build TurnInput")?;

    // Execute turn with timing
    let start = Instant::now();
    let output = execute_turn(input, TurnConfig::default()).await?;
    let duration = start.elapsed();

    // Render output
    println!();
    println!("{}", "═".repeat(60).dimmed());
    println!("{}", " Turn Execution Results ".bold().cyan());
    println!("{}", "═".repeat(60).dimmed());
    println!();

    // Message history
    println!("{}", "Messages:".bold());
    for msg in &output.messages {
        let role_str = match msg.role {
            claw::Role::User => "USER",
            claw::Role::Assistant => "ASSISTANT",
            claw::Role::System => "SYSTEM",
            claw::Role::Tool => "TOOL",
        };
        let role_colored = match msg.role {
            claw::Role::User => role_str.blue().to_string(),
            claw::Role::Assistant => role_str.green().to_string(),
            claw::Role::System => role_str.yellow().to_string(),
            claw::Role::Tool => role_str.magenta().to_string(),
        };
        let content = if verbose {
            msg.content.clone()
        } else {
            // Truncate to 100 chars for non-verbose
            if msg.content.len() > 100 {
                format!("{}...", &msg.content[..100])
            } else {
                msg.content.clone()
            }
        };
        println!("  {role_colored} {content}");

        if verbose && let Some(tool_calls) = &msg.tool_calls {
            for tc in tool_calls {
                let args = tc.arguments.to_string();
                println!("    {}({})", tc.name.cyan(), args.dimmed());
            }
        }
    }

    println!();

    // Token usage table
    println!("{}", "Token Usage:".bold());
    println!(
        "  {:<15} {}",
        "Input Tokens".dimmed(),
        output.token_usage.input_tokens
    );
    println!(
        "  {:<15} {}",
        "Output Tokens".dimmed(),
        output.token_usage.output_tokens
    );
    println!(
        "  {:<15} {}",
        "Total Tokens".dimmed(),
        output.token_usage.total_tokens
    );
    println!();

    // Summary
    println!("{}", "Summary:".bold());
    println!("  Duration: {duration:?}");
    println!("  Messages: {}", output.messages.len());

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::TurnCommand;
    use crate::dev::DevCli;
    use crate::dev::DevCommand;
    use clap::Parser;

    #[test]
    fn parses_turn_test_command_with_all_options() {
        let cli = DevCli::parse_from([
            "cli",
            "turn",
            "test",
            "hello",
            "--provider",
            "openai",
            "--tools",
            "echo,http",
            "--system-prompt",
            "Be helpful",
            "--input",
            "prompt.md",
            "--verbose",
        ]);

        match cli.command {
            DevCommand::Turn(TurnCommand::Test {
                provider,
                tools,
                system_prompt,
                message,
                input,
                verbose,
            }) => {
                assert_eq!(provider, Some("openai".to_string()));
                assert_eq!(tools, vec!["echo".to_string(), "http".to_string()]);
                assert_eq!(system_prompt, "Be helpful");
                assert_eq!(message, "hello");
                assert_eq!(input, Some("prompt.md".to_string()));
                assert!(verbose);
            }
            _ => panic!("turn test command should parse"),
        }
    }

    #[test]
    fn parses_turn_test_command_with_defaults() {
        let cli = DevCli::parse_from(["cli", "turn", "test"]);

        match cli.command {
            DevCommand::Turn(TurnCommand::Test {
                provider,
                tools,
                system_prompt,
                message,
                input,
                verbose,
            }) => {
                assert_eq!(provider, None);
                assert!(tools.is_empty());
                assert_eq!(system_prompt, "You are a helpful assistant.");
                assert_eq!(message, "Hello");
                assert_eq!(input, None);
                assert!(!verbose);
            }
            _ => panic!("turn test command should parse"),
        }
    }
}
