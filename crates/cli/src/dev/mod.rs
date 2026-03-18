//! Dev commands module - entry point for arguswing-dev.
//!
//! This module aggregates all development-only commands:
//! - llm: LLM completion testing
//! - turn: Agent/LLM turn execution testing
//! - approval: Approval flow testing
//! - workflow: Workflow management testing
//! - thread: Thread management testing

pub mod approval;
pub mod config;
pub mod llm;
pub mod turn;
pub mod workflow;

use anyhow::Result;
use clap::{Parser, Subcommand};
use argus_wing::ArgusWing;
use sqlx::migrate::Migrator;
use std::sync::Arc;

use crate::dev::approval::ApprovalCommand;
use crate::dev::llm::LlmCommand;
use crate::dev::turn::TurnCommand;
use crate::dev::workflow::WorkflowCommand;
use crate::provider::ProviderCommand;

/// Approval dev migrator for CLI testing.
pub static APPROVAL_DEV_MIGRATOR: Migrator = sqlx::migrate!("./src/dev/migrations");

/// Dev CLI for arguswing-dev.
#[derive(Debug, Parser)]
pub struct DevCli {
    #[command(subcommand)]
    pub command: DevCommand,
}

/// 开发与测试命令，用于 LLM 提供商、Agent 和工作流。
#[derive(Debug, Subcommand)]
pub enum DevCommand {
    /// 管理 LLM 提供商配置。
    #[command(subcommand)]
    Provider(ProviderCommand),
    /// 测试 LLM 补全请求。
    #[command(subcommand)]
    Llm(LlmCommand),
    /// 测试 Agent/LLM Turn 执行流程。
    #[command(subcommand)]
    Turn(TurnCommand),
    /// 管理审批请求和响应。
    #[command(subcommand)]
    Approval(ApprovalCommand),
    /// 管理工作流、阶段和任务。
    #[command(subcommand)]
    Workflow(WorkflowCommand),
    /// 管理对话线程 (开发测试)。
    #[command(subcommand)]
    Thread(ThreadCommand),
}

/// Thread commands for dev CLI (placeholder).
#[derive(Debug, Subcommand)]
pub enum ThreadCommand {
    /// Start a new thread with a message.
    Start {
        /// Provider to use.
        #[arg(long)]
        provider: String,
        /// Initial message.
        #[arg(long)]
        message: String,
        /// System prompt.
        #[arg(long)]
        system: Option<String>,
    },
    /// List all threads.
    List,
    /// Continue an existing thread.
    Continue {
        /// Thread ID.
        #[arg(long)]
        id: String,
        /// Message to send.
        #[arg(long)]
        message: String,
    },
}

/// Run dev CLI.
pub async fn run(wing: Arc<ArgusWing>, command: DevCommand) -> Result<()> {
    match command {
        DevCommand::Provider(cmd) => crate::provider::run_provider_command(wing, cmd).await,
        DevCommand::Llm(cmd) => crate::dev::llm::run_llm_command(wing, cmd).await,
        DevCommand::Turn(cmd) => crate::dev::turn::run_turn_command(wing, cmd).await,
        DevCommand::Approval(cmd) => crate::dev::approval::run_approval_command(cmd).await,
        DevCommand::Workflow(cmd) => crate::dev::workflow::run_workflow_command(wing, cmd).await,
        DevCommand::Thread(cmd) => run_thread_command(wing, cmd).await,
    }
}

/// Run thread command (placeholder).
async fn run_thread_command(_wing: Arc<ArgusWing>, command: ThreadCommand) -> Result<()> {
    match command {
        ThreadCommand::Start {
            provider,
            message,
            system,
        } => {
            eprintln!("Thread start not yet implemented");
            eprintln!("  Provider: {provider}");
            eprintln!("  Message: {message}");
            if let Some(sys) = system {
                eprintln!("  System: {sys}");
            }
        }
        ThreadCommand::List => {
            eprintln!("Thread list not yet implemented");
        }
        ThreadCommand::Continue { id, message } => {
            eprintln!("Thread continue not yet implemented");
            eprintln!("  Thread ID: {id}");
            eprintln!("  Message: {message}");
        }
    }
    Ok(())
}

/// Try to run dev CLI if a dev command is detected.
pub async fn try_run(wing: Arc<ArgusWing>) -> Result<bool> {
    let Some(first_arg) = std::env::args().nth(1) else {
        return Ok(false);
    };
    if !matches!(
        first_arg.as_str(),
        "provider" | "llm" | "turn" | "approval" | "workflow" | "thread"
    ) {
        return Ok(false);
    }

    let cli = DevCli::parse();
    run(wing, cli.command).await?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::{DevCli, DevCommand};
    use crate::provider::ProviderCommand;
    use clap::Parser;

    #[test]
    fn parses_provider_import_command() {
        let cli = DevCli::parse_from(["cli", "provider", "import", "--file", "./providers.toml"]);

        match cli.command {
            DevCommand::Provider(ProviderCommand::Import { file }) => {
                assert_eq!(file, "./providers.toml");
            }
            _ => panic!("provider import command should parse"),
        }
    }
}
