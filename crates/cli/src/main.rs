use std::env;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use agent::Agent;
use agent::db::sqlite::{SqliteLlmProviderRepository, connect, connect_path, migrate};
use agent::llm::LLMManager;
use anyhow::{Context, Result, anyhow};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let database_target = resolve_database_target()?;
    let pool = match &database_target {
        DatabaseTarget::Url(database_url) => connect(database_url).await,
        DatabaseTarget::Path(path) => {
            ensure_parent_dir(path)?;
            connect_path(path).await
        }
    }?;
    migrate(&pool).await?;

    let repository = Arc::new(SqliteLlmProviderRepository::new(pool));
    let llm_manager = Arc::new(LLMManager::new(repository));
    let agent = Agent::new(llm_manager);
    let provider_count = agent.llm_manager().list_providers().await?.len();

    tracing::info!(provider_count, "argusclaw initialized");

    Ok(())
}

fn init_tracing() {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("argusclaw=info,agent=info"));

    tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .with_target(false)
        .compact()
        .init();
}

enum DatabaseTarget {
    Url(String),
    Path(PathBuf),
}

fn resolve_database_target() -> Result<DatabaseTarget> {
    let configured =
        env::var("DATABASE_URL").unwrap_or_else(|_| "~/.argusclaw/sqlite.db".to_string());

    if configured.starts_with("sqlite:") {
        return Ok(DatabaseTarget::Url(configured));
    }

    Ok(DatabaseTarget::Path(expand_home_path(&configured)?))
}

fn expand_home_path(path: &str) -> Result<PathBuf> {
    if let Some(relative_path) = path.strip_prefix("~/") {
        let home_dir =
            dirs::home_dir().ok_or_else(|| anyhow!("failed to resolve home directory"))?;
        return Ok(home_dir.join(relative_path));
    }

    Ok(PathBuf::from(path))
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("database path `{}` has no parent directory", path.display()))?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("failed to create database directory `{}`", parent.display()))?;

    Ok(())
}
