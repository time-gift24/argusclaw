use std::path::{Path, PathBuf};

use argus_server::server_config::{EncryptedConfigValue, LoggingConfig, ServerConfig};
use clap::{Parser, Subcommand};
use tracing_appender::non_blocking::WorkerGuard;
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

#[derive(Debug, Parser)]
#[command(name = "argus-server")]
struct Cli {
    #[arg(long, global = true)]
    config: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Config {
        #[command(subcommand)]
        command: ConfigCommand,
    },
}

#[derive(Debug, Subcommand)]
enum ConfigCommand {
    Encrypt {
        #[arg(long)]
        value: String,
    },
}

fn init_tracing(
    logging: &LoggingConfig,
) -> Result<Option<WorkerGuard>, Box<dyn std::error::Error>> {
    let rust_log = std::env::var("RUST_LOG").ok();
    let level = logging
        .level
        .as_deref()
        .or(rust_log.as_deref())
        .unwrap_or("info")
        .to_string();
    let filter = EnvFilter::new(level);

    if let Some(file_path) = &logging.file_path {
        let parent = file_path.parent().unwrap_or_else(|| Path::new("."));
        std::fs::create_dir_all(parent)?;
        let file_name = file_path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("argus-server.log");
        let appender = tracing_appender::rolling::never(parent, file_name);
        let (writer, guard) = tracing_appender::non_blocking(appender);
        tracing_subscriber::registry()
            .with(filter)
            .with(
                fmt::layer()
                    .with_target(true)
                    .with_ansi(false)
                    .with_writer(writer),
            )
            .init();
        return Ok(Some(guard));
    }

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(true))
        .init();
    Ok(None)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cli = Cli::parse();
    let loaded = ServerConfig::load(cli.config.as_deref())?;

    if let Some(Command::Config {
        command: ConfigCommand::Encrypt { value },
    }) = cli.command
    {
        let encrypted = EncryptedConfigValue::encrypt_with_master_key(
            loaded.config.master_key_path.display().to_string(),
            &value,
        )?;
        println!(
            "{{ encrypted = \"{}\", nonce = \"{}\" }}",
            encrypted.encrypted, encrypted.nonce
        );
        return Ok(());
    }

    let _log_guard = init_tracing(&loaded.config.logging)?;
    tracing::info!(
        config_source = %loaded.source,
        bind_addr = %loaded.config.bind_addr,
        web_dist_dir = loaded
            .config
            .web_dist_dir
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<disabled>".to_string()),
        trace_dir = %loaded.config.trace_dir.display(),
        auth_enabled = loaded.config.auth.dev_enabled || loaded.config.auth.oauth.enabled,
        auth_dev_enabled = loaded.config.auth.dev_enabled,
        auth_oauth_enabled = loaded.config.auth.oauth.enabled,
        log_file = loaded
            .config
            .logging
            .file_path
            .as_ref()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<journald/stdout>".to_string()),
        "starting argus-server"
    );

    let app = argus_server::build_app_with_config(None, &loaded.config).await?;
    let listener = tokio::net::TcpListener::bind(loaded.config.bind_addr).await?;
    tracing::info!(bind_addr = %loaded.config.bind_addr, "argus-server listening");
    axum::serve(listener, app).await?;
    Ok(())
}
