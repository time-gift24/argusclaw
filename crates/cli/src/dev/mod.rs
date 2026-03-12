//! Dev commands module - entry point for argusclaw-dev.
//!
//! This module aggregates all development-only commands:
//! - llm: LLM completion testing
//! - turn: Agent/LLM turn execution testing
//! - approval: Approval flow testing
//! - workflow: Workflow management testing

pub mod approval;
pub mod config;
pub mod llm;
pub mod turn;
pub mod workflow;

use anyhow::Result;
use clap::{Parser, Subcommand};
use claw::AppContext;
use sqlx::migrate::Migrator;

use crate::dev::approval::ApprovalCommand;
use crate::dev::llm::LlmCommand;
use crate::dev::turn::TurnCommand;
use crate::dev::workflow::WorkflowCommand;
use crate::provider::ProviderCommand;

/// Approval dev migrator for CLI testing.
pub static APPROVAL_DEV_MIGRATOR: Migrator = sqlx::migrate!("./src/dev/migrations");

/// Dev CLI for argusclaw-dev.
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
}

/// Run dev CLI.
pub async fn run(ctx: AppContext, command: DevCommand) -> Result<()> {
    match command {
        DevCommand::Provider(cmd) => crate::provider::run_provider_command(ctx, cmd).await,
        DevCommand::Llm(cmd) => crate::dev::llm::run_llm_command(ctx, cmd).await,
        DevCommand::Turn(cmd) => crate::dev::turn::run_turn_command(ctx, cmd).await,
        DevCommand::Approval(cmd) => crate::dev::approval::run_approval_command(cmd).await,
        DevCommand::Workflow(cmd) => crate::dev::workflow::run_workflow_command(ctx, cmd).await,
    }
}

/// Try to run dev CLI if a dev command is detected.
pub async fn try_run(ctx: AppContext) -> Result<bool> {
    let Some(first_arg) = std::env::args().nth(1) else {
        return Ok(false);
    };
    if !matches!(
        first_arg.as_str(),
        "provider" | "llm" | "turn" | "approval" | "workflow"
    ) {
        return Ok(false);
    }

    let cli = DevCli::parse();
    run(ctx, cli.command).await?;
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
