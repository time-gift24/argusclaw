use std::env;

#[cfg(feature = "dev")]
mod dev;

use anyhow::Result;
use claw::AppContext;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let ctx = AppContext::init(resolve_database_target_for_startup()?).await?;

    #[cfg(feature = "dev")]
    if dev::try_run(ctx.clone()).await? {
        return Ok(());
    }

    let provider_count = ctx.llm_manager().list_providers().await?.len();

    tracing::info!(provider_count, "argusclaw initialized");

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

fn resolve_database_target_for_startup() -> Result<Option<String>> {
    if let Ok(database_url) = env::var("DATABASE_URL") {
        return Ok(Some(database_url));
    }

    #[cfg(feature = "dev")]
    {
        if let Some(first_arg) = env::args().nth(1)
            && matches!(first_arg.as_str(), "provider" | "llm" | "turn" | "thread" | "approval" | "workflow")
        {
            let tmp_dir = env::current_dir()?.join("tmp");
            std::fs::create_dir_all(&tmp_dir)?;
            let db_path = tmp_dir.join("cli-dev.sqlite");
            return Ok(Some(db_path.display().to_string()));
        }
    }

    Ok(None)
}
