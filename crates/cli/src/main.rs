//! Production CLI entry point (arguswing).
//!
//! This binary is minimal - it only includes provider and agent commands.

use anyhow::Result;
use clap::{Parser, Subcommand};
use claw::AppContext;
use tracing_subscriber::EnvFilter;

use cli::agent::{AgentCommand, run_agent_command};
use cli::provider::{ProviderCommand, run_provider_command};
use cli::{db_path_to_url, resolve_db_path};

/// ArgusWing - AI Agent CLI Tool
#[derive(Debug, Parser)]
#[command(name = "arguswing", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// LLM provider management
    #[command(subcommand)]
    Provider(ProviderCommand),
    /// Agent commands for interactive conversations
    #[command(subcommand)]
    Agent(AgentCommand),
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    // Resolve database path for production CLI
    let db_path = resolve_db_path(false);
    let db_url = db_path_to_url(&db_path);

    // Initialize app context
    let ctx = AppContext::init(Some(db_url)).await?;

    // Parse CLI
    let cli = Cli::parse();

    match cli.command {
        Command::Provider(cmd) => run_provider_command(ctx, cmd).await?,
        Command::Agent(cmd) => run_agent_command(ctx, cmd).await?,
    }

    Ok(())
}

fn init_tracing() {
    // Only initialize tracing if RUST_LOG is set
    if std::env::var("RUST_LOG").is_ok() {
        let env_filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("arguswing=info,claw=info"));

        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_target(false)
            .with_writer(std::io::stderr)
            .init();
    }
}
