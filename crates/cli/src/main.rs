//! Production CLI entry point (argusclaw).
//!
//! This binary is minimal - it only includes provider and thread commands.

use anyhow::Result;
use clap::{Parser, Subcommand};
use claw::AppContext;
use tracing_subscriber::EnvFilter;

use cli::provider::{ProviderCommand, run_provider_command};
use cli::{db_path_to_url, resolve_db_path};

/// ArgusClaw - AI Agent CLI Tool
#[derive(Debug, Parser)]
#[command(name = "argusclaw", version, about)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// LLM provider management
    #[command(subcommand)]
    Provider(ProviderCommand),
    /// Thread management (placeholder)
    Thread {
        #[command(subcommand)]
        command: ThreadCommand,
    },
}

/// Thread commands (placeholder - not yet implemented)
#[derive(Debug, Subcommand)]
enum ThreadCommand {
    /// Start a new thread
    Start,
    /// List all threads
    List,
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
        Command::Thread { command } => match command {
            ThreadCommand::Start | ThreadCommand::List => {
                eprintln!("Thread commands are not yet implemented");
                eprintln!("Use 'argusclaw-dev thread' for development testing");
            }
        },
    }

    Ok(())
}

fn init_tracing() {
    // Only initialize tracing if RUST_LOG is set
    if std::env::var("RUST_LOG").is_ok() {
        let env_filter = EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| EnvFilter::new("argusclaw=info,claw=info"));

        tracing_subscriber::fmt()
            .with_env_filter(env_filter)
            .with_target(false)
            .with_writer(std::io::stderr)
            .init();
    }
}
