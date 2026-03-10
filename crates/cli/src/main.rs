use std::env;

#[cfg(feature = "dev")]
mod dev;

use agent::Agent;
use anyhow::Result;
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();

    let agent = Agent::init(env::var("DATABASE_URL").ok()).await?;

    #[cfg(feature = "dev")]
    if dev::try_run(agent.clone()).await? {
        return Ok(());
    }

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
