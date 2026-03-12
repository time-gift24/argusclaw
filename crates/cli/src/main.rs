//! Production CLI entry point (argusclaw).
//!
//! This binary is minimal - it only includes provider and thread commands.

use std::env;

use anyhow::Result;
use claw::AppContext;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    // Resolve database path for production CLI
    let db_path = resolve_db_path(false);
    let db_url = db_path_to_url(&db_path);

    // Initialize app context
    let ctx = AppContext::init(Some(db_url)).await?;

    let provider_count = ctx.llm_manager().list_providers().await?.len();

    tracing::info!(provider_count, "argusclaw initialized");

    Ok(())
}

/// Resolve database path based on CLI mode (production vs development).
///
/// # Production (argusclaw)
/// - Default: `~/.argusclaw/sqlite.db`
/// - Override: `ARGUSCLAW_DB` environment variable
///
/// # Development (argusclaw-dev)
/// - Default: `./tmp/argusclaw-dev.db`
/// - Override: `ARGUSCLAW_DEV_DB` environment variable
fn resolve_db_path(is_dev: bool) -> std::path::PathBuf {
    let (env_var, default_path) = if is_dev {
        ("ARGUSCLAW_DEV_DB", {
            let cwd = env::current_dir().expect("failed to resolve current working directory");
            let tmp_dir = cwd.join("tmp");
            std::fs::create_dir_all(&tmp_dir).expect("failed to create tmp directory");
            tmp_dir.join("argusclaw-dev.db")
        })
    } else {
        ("ARGUSCLAW_DB", {
            let home = dirs::home_dir().expect("failed to resolve home directory");
            let data_dir = home.join(".argusclaw");
            std::fs::create_dir_all(&data_dir).expect("failed to create .argusclaw directory");
            data_dir.join("sqlite.db")
        })
    };

    if let Ok(value) = env::var(env_var) {
        if let Some(stripped) = value.strip_prefix("sqlite:") {
            std::path::PathBuf::from(stripped)
        } else {
            std::path::PathBuf::from(value)
        }
    } else {
        default_path
    }
}

/// Convert database path to connection URL format.
fn db_path_to_url(path: &std::path::Path) -> String {
    format!("sqlite:{}", path.display())
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
