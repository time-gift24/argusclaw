use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, Result, anyhow};
use argus_agent::{LlmThreadCompactor, ThreadBuilder, TurnCancellation};
use argus_llm::{Cipher, FileKeySource, ProviderManager};
use argus_protocol::llm::{ChatMessage, LlmProviderId, LlmStreamEvent, Role};
use argus_protocol::{AgentId, AgentRecord, AgentType, ProviderId, SessionId, ThreadEvent};
use argus_repository::traits::AccountRepository;
use argus_repository::{ArgusSqlite, connect, connect_path, migrate};
use clap::Parser;
use sqlx::SqlitePool;
use tokio::sync::broadcast;

#[derive(Debug, Clone, PartialEq, Eq)]
enum DatabaseTarget {
    Url(String),
    Path(PathBuf),
}

#[derive(Debug, Parser)]
#[command(
    name = "argus-job-smoke-chat",
    version,
    about = "Use the configured default LLM model to run a basic send/receive turn"
)]
struct Cli {
    #[arg(long)]
    database: Option<String>,
    #[arg(long)]
    prompt: String,
    #[arg(long)]
    system_prompt: Option<String>,
    #[arg(long, default_value_t = false)]
    stream: bool,
}

fn resolve_database_target(
    configured: Option<&str>,
    database_url: Option<&str>,
    home_dir: Option<&Path>,
) -> anyhow::Result<DatabaseTarget> {
    let configured = configured
        .map(str::to_owned)
        .or_else(|| database_url.map(str::to_owned))
        .unwrap_or_else(|| "~/.arguswing/sqlite.db".to_string());

    if configured.starts_with("sqlite:") {
        return Ok(DatabaseTarget::Url(configured));
    }

    Ok(DatabaseTarget::Path(expand_home_path(
        &configured,
        home_dir,
    )?))
}

fn extract_last_assistant_message(messages: &[ChatMessage]) -> Option<String> {
    messages
        .iter()
        .rev()
        .find(|message| message.role == Role::Assistant)
        .and_then(|message| {
            if !message.content.trim().is_empty() {
                Some(message.content.clone())
            } else {
                message.reasoning_content.clone()
            }
        })
}

fn expand_home_path(path: &str, home_dir: Option<&Path>) -> Result<PathBuf> {
    if let Some(relative_path) = path.strip_prefix("~/") {
        let resolved_home = home_dir
            .map(Path::to_path_buf)
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
            .ok_or_else(|| anyhow!("Cannot determine home directory for {}", path))?;
        return Ok(resolved_home.join(relative_path));
    }

    Ok(PathBuf::from(path))
}

fn ensure_parent_dir(path: &Path) -> Result<()> {
    let parent = path
        .parent()
        .ok_or_else(|| anyhow!("Invalid database path: {}", path.display()))?;
    std::fs::create_dir_all(parent)
        .with_context(|| format!("Cannot create database directory {}", parent.display()))?;
    Ok(())
}

async fn connect_database(target: &DatabaseTarget) -> Result<SqlitePool> {
    let pool = match target {
        DatabaseTarget::Url(url) => connect(url)
            .await
            .with_context(|| format!("Failed to connect to {}", url))?,
        DatabaseTarget::Path(path) => {
            ensure_parent_dir(path)?;
            connect_path(path)
                .await
                .with_context(|| format!("Failed to connect to {}", path.display()))?
        }
    };
    migrate(&pool).await.context("Failed to run migrations")?;
    Ok(pool)
}

fn build_smoke_agent_record(
    provider_id: LlmProviderId,
    system_prompt: Option<String>,
) -> Arc<AgentRecord> {
    Arc::new(AgentRecord {
        id: AgentId::new(0),
        display_name: "Job Smoke Chat".to_string(),
        description: "Smoke test agent for default provider chat".to_string(),
        version: "0.1.0".to_string(),
        provider_id: Some(ProviderId::new(provider_id.into_inner())),
        model_id: None,
        system_prompt: system_prompt.unwrap_or_default(),
        tool_names: Vec::new(),
        max_tokens: None,
        temperature: None,
        thinking_config: None,
        parent_agent_id: None,
        agent_type: AgentType::Standard,
        is_enabled: true,
    })
}

async fn stream_turn_events(mut rx: broadcast::Receiver<ThreadEvent>) {
    while let Ok(event) = rx.recv().await {
        if let ThreadEvent::Processing {
            event: LlmStreamEvent::ContentDelta { delta },
            ..
        } = event
        {
            print!("{delta}");
            let _ = io::stdout().flush();
        }
    }
}

async fn run(cli: Cli) -> Result<()> {
    let database_target = resolve_database_target(
        cli.database.as_deref(),
        std::env::var("DATABASE_URL").ok().as_deref(),
        std::env::var_os("HOME").as_deref().map(Path::new),
    )?;
    let pool = connect_database(&database_target).await?;
    let cipher = Arc::new(Cipher::new(FileKeySource::from_env_or_default()));
    let repository = Arc::new(ArgusSqlite::new(pool.clone()));
    let account_repo = repository.clone() as Arc<dyn AccountRepository>;
    let provider_manager =
        ProviderManager::new(repository).with_auth(account_repo, Arc::clone(&cipher));

    let provider_record = provider_manager
        .get_default_provider_record()
        .await
        .context("Failed to load default provider record")?;
    let provider = provider_manager
        .get_default_provider()
        .await
        .context("Failed to construct default provider")?;

    println!(
        "Using provider {} ({}) with model {}",
        provider_record.display_name, provider_record.id, provider_record.default_model
    );

    let mut thread = ThreadBuilder::new()
        .provider(Arc::clone(&provider))
        .compactor(Arc::new(LlmThreadCompactor::new(provider)))
        .agent_record(build_smoke_agent_record(
            provider_record.id,
            cli.system_prompt.clone(),
        ))
        .session_id(SessionId::new())
        .build()
        .context("Failed to build smoke thread")?;
    let stream_rx = cli.stream.then(|| thread.subscribe());

    let stream_task = stream_rx.map(|rx| tokio::spawn(stream_turn_events(rx)));
    let output_result = thread
        .execute_turn(cli.prompt, None, TurnCancellation::new())
        .await
        .context("Smoke turn failed");
    if let Some(handle) = stream_task {
        handle.abort();
        let _ = handle.await;
        if output_result.is_ok() {
            println!();
        }
    }
    let record = output_result?;
    let committed_messages: Vec<_> = thread.history_iter().cloned().collect();

    let reply = extract_last_assistant_message(&committed_messages)
        .ok_or_else(|| anyhow!("Turn completed without an assistant reply"))?;

    if !cli.stream {
        println!("\nAssistant:\n{reply}");
    }
    println!(
        "\nToken usage: input={}, output={}, total={}",
        record.token_usage.input_tokens,
        record.token_usage.output_tokens,
        record.token_usage.total_tokens
    );
    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    run(Cli::parse()).await
}

#[cfg(test)]
mod tests {
    use super::*;
    use argus_protocol::llm::Role;

    #[test]
    fn resolve_database_target_prefers_explicit_sqlite_url() {
        let resolved = resolve_database_target(
            Some("sqlite::memory:"),
            Some("/ignored/by-explicit"),
            Some(Path::new("/tmp/home")),
        )
        .expect("sqlite URL should resolve");

        assert_eq!(resolved, DatabaseTarget::Url("sqlite::memory:".to_string()));
    }

    #[test]
    fn resolve_database_target_expands_default_home_path() {
        let resolved = resolve_database_target(None, None, Some(Path::new("/tmp/home")))
            .expect("default path should resolve");

        assert_eq!(
            resolved,
            DatabaseTarget::Path(PathBuf::from("/tmp/home/.arguswing/sqlite.db"))
        );
    }

    #[test]
    fn extract_last_assistant_message_prefers_latest_assistant_content() {
        let messages = vec![
            ChatMessage::user("hello"),
            ChatMessage::assistant("first reply"),
            ChatMessage {
                role: Role::Assistant,
                content: String::new(),
                reasoning_content: Some("hidden reasoning".to_string()),
                content_parts: Vec::new(),
                tool_call_id: None,
                name: None,
                tool_calls: None,
                metadata: None,
            },
        ];

        let reply = extract_last_assistant_message(&messages);

        assert_eq!(reply.as_deref(), Some("hidden reasoning"));
    }
}
