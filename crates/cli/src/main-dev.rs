//! Development CLI entry point (argusclaw-dev).
//!
//! This binary includes all development-only commands and is a superset of
//! the production CLI.

use anyhow::Result;
use clap::Parser;
use claw::AppContext;
use tracing_subscriber::EnvFilter;

use cli::dev::{DevCli, run};

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    // Resolve database path for dev CLI
    let db_path = cli::resolve_db_path(true);
    let db_url = cli::db_path_to_url(&db_path);

    // Initialize app context
    let ctx = AppContext::init(Some(db_url)).await?;

    // Parse dev CLI
    let cli = DevCli::parse();
    run(ctx, cli.command).await
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
