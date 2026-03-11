//! Provider management commands for LLM providers.
//!
//! This is a production command for managing LLM providers.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use clap::{Args, FromArgMatches, Subcommand};

use claw::db::llm::{
    LlmProviderId, LlmProviderKind, LlmProviderRecord, LlmProviderSummary, SecretString,
};
use claw::AppContext;
use owo_colors::OwoColorize;

/// Provider commands for managing LLM providers.
#[derive(Debug, Subcommand)]
pub enum ProviderCommand {
    /// Import providers from a TOML file.
    Import {
        /// Path to the TOML file containing provider configurations.
        #[arg(long)]
        file: String,
    },

    /// List all configured providers.
    List,

    /// Get details of a specific provider.
    Get {
        /// Provider ID to look up.
        #[arg(long)]
        id: String,
    },

    /// Create or update a provider.
    Upsert(ProviderUpsertArgs),

    /// Set a provider as the default.
    SetDefault {
        /// Provider ID to set as default.
        #[arg(long)]
        id: String,
    },

    /// Get the currently configured default provider.
    GetDefault,
}

/// Arguments for creating or updating a provider.
#[derive(Debug, Args)]
pub struct ProviderUpsertArgs {
    /// Unique identifier for this provider.
    #[arg(long)]
    pub id: String,

    /// Human-readable display name.
    #[arg(long = "display-name")]
    pub display_name: String,

    /// Provider kind (e.g., "openai-compatible").
    #[arg(long)]
    pub kind: String,

    /// Base URL for the provider API.
    #[arg(long = "base-url")]
    pub base_url: String,

    /// API key for authentication.
    #[arg(long = "api-key")]
    pub api_key: String,

    /// Model identifier to use.
    #[arg(long)]
    pub model: String,

    /// Set this provider as the default.
    #[arg(long = "default", default_value_t = false)]
    pub is_default: bool,
}

/// Display record for provider output (hides sensitive data).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderDisplayRecord {
    pub id: String,
    pub display_name: String,
    pub kind: String,
    pub base_url: String,
    pub model: String,
    pub is_default: bool,
}

impl From<LlmProviderSummary> for ProviderDisplayRecord {
    fn from(value: LlmProviderSummary) -> Self {
        Self {
            id: value.id.to_string(),
            display_name: value.display_name,
            kind: value.kind.to_string(),
            base_url: value.base_url,
            model: value.model,
            is_default: value.is_default,
        }
    }
}

impl From<LlmProviderRecord> for ProviderDisplayRecord {
    fn from(value: LlmProviderRecord) -> Self {
        Self {
            id: value.id.to_string(),
            display_name: value.display_name,
            kind: value.kind.to_string(),
            base_url: value.base_url,
            model: value.model,
            is_default: value.is_default,
        }
    }
}

impl TryFrom<ProviderUpsertArgs> for LlmProviderRecord {
    type Error = claw::db::DbError;

    fn try_from(value: ProviderUpsertArgs) -> Result<Self, Self::Error> {
        Ok(Self {
            id: LlmProviderId::new(value.id),
            kind: value.kind.parse::<LlmProviderKind>()?,
            display_name: value.display_name,
            base_url: value.base_url,
            api_key: SecretString::new(value.api_key),
            model: value.model,
            is_default: value.is_default,
        })
    }
}

/// Try to run a provider command if the first arg matches.
///
/// Returns `Ok(true)` if the command was handled, `Ok(false)` otherwise.
pub async fn try_run(ctx: AppContext) -> Result<bool> {
    let Some(first_arg) = std::env::args().nth(1) else {
        return Ok(false);
    };

    if first_arg != "provider" {
        return Ok(false);
    }

    let args: Vec<String> = std::env::args().skip(1).collect();
    let matches = ProviderCommand::augment_subcommands(clap::Command::new("argusclaw provider"))
        .try_get_matches_from(&args)?;

    let command = ProviderCommand::from_arg_matches(&matches)?;
    run(ctx, command).await?;
    Ok(true)
}

/// Run a provider command.
pub async fn run(ctx: AppContext, command: ProviderCommand) -> Result<()> {
    match command {
        ProviderCommand::Import { file } => run_import(ctx, file).await,
        ProviderCommand::List => run_list(ctx).await,
        ProviderCommand::Get { id } => run_get(ctx, id).await,
        ProviderCommand::Upsert(args) => run_upsert(ctx, args).await,
        ProviderCommand::SetDefault { id } => run_set_default(ctx, id).await,
        ProviderCommand::GetDefault => run_get_default(ctx).await,
    }
}

async fn run_import(ctx: AppContext, file: String) -> Result<()> {
    let contents = std::fs::read_to_string(Path::new(&file))
        .with_context(|| format!("failed to read provider import file `{file}`"))?;

    let config: ProviderImportFile =
        toml::from_str(&contents).context("failed to parse provider import toml")?;

    let records = config.into_records().map_err(|e| anyhow!(e.to_string()))?;
    ctx.import_providers(records).await?;

    println!("{} Providers imported successfully", "✓".green());
    Ok(())
}

async fn run_list(ctx: AppContext) -> Result<()> {
    let providers = ctx.llm_manager().list_providers().await?;

    if providers.is_empty() {
        println!("No providers configured.");
        println!();
        println!("Use '{}' to add a provider.", "provider upsert".cyan());
        println!("Or use '{}' to import from a TOML file.", "provider import --file <path>".cyan());
        return Ok(());
    }

    println!("Configured providers:");
    println!();

    for provider in providers {
        let record: ProviderDisplayRecord = provider.into();
        println!("{}", render_provider_output(&record));
        println!();
    }

    Ok(())
}

async fn run_get(ctx: AppContext, id: String) -> Result<()> {
    let provider = ctx
        .get_provider_record(&LlmProviderId::new(id))
        .await
        .map_err(|e| anyhow!("Provider not found: {}", e))?;

    println!("{}", render_provider_output(&provider.into()));
    Ok(())
}

async fn run_upsert(ctx: AppContext, args: ProviderUpsertArgs) -> Result<()> {
    let record = LlmProviderRecord::try_from(args).map_err(|e| anyhow!(e.to_string()))?;
    let is_new = ctx
        .get_provider_record(&record.id)
        .await
        .is_ok();

    ctx.upsert_provider(record.clone()).await?;

    if is_new {
        println!("{} Provider '{}' created", "✓".green(), record.id);
    } else {
        println!("{} Provider '{}' updated", "✓".green(), record.id);
    }

    if record.is_default {
        println!("  Set as default provider");
    }

    Ok(())
}

async fn run_set_default(ctx: AppContext, id: String) -> Result<()> {
    ctx.set_default_provider(&LlmProviderId::new(&id))
        .await
        .map_err(|e| anyhow!("Failed to set default provider: {}", e))?;

    println!("{} Default provider set to '{}'", "✓".green(), id);
    Ok(())
}

async fn run_get_default(ctx: AppContext) -> Result<()> {
    let provider = ctx
        .get_default_provider_record()
        .await
        .map_err(|e| anyhow!("No default provider configured: {}", e))?;

    println!("{}", render_provider_output(&provider.into()));
    Ok(())
}

/// Render provider for display (hides API key).
pub fn render_provider_output(record: &ProviderDisplayRecord) -> String {
    let default_str = if record.is_default {
        "yes".green().to_string()
    } else {
        "no".dimmed().to_string()
    };

    format!(
        "{} {}\n  {} {}\n  {} {}\n  {} {}\n  {} {}\n  {} {}",
        "id:".dimmed(),
        record.id.cyan(),
        "display_name:".dimmed(),
        record.display_name,
        "kind:".dimmed(),
        record.kind.yellow(),
        "base_url:".dimmed(),
        record.base_url,
        "model:".dimmed(),
        record.model,
        "is_default:".dimmed(),
        default_str
    )
}

// ---------------------------------------------------------------------------
// Provider Import File Parser
// ---------------------------------------------------------------------------

/// Provider import file structure.
#[derive(Debug, Clone, serde::Deserialize)]
struct ProviderImportFile {
    providers: Vec<ProviderImport>,
}

/// Single provider import record.
#[derive(Debug, Clone, serde::Deserialize)]
struct ProviderImport {
    id: String,
    display_name: String,
    kind: String,
    base_url: String,
    api_key: String,
    model: String,
    #[serde(default)]
    is_default: bool,
}

impl ProviderImportFile {
    fn into_records(self) -> Result<Vec<LlmProviderRecord>, String> {
        self.providers
            .into_iter()
            .map(|p| {
                let kind = p.kind.parse::<LlmProviderKind>().map_err(|e| e.to_string())?;
                Ok(LlmProviderRecord {
                    id: LlmProviderId::new(p.id),
                    kind,
                    display_name: p.display_name,
                    base_url: p.base_url,
                    api_key: SecretString::new(p.api_key),
                    model: p.model,
                    is_default: p.is_default,
                })
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rendered_provider_output_hides_api_keys() {
        let output = render_provider_output(&ProviderDisplayRecord {
            id: "openai".to_string(),
            display_name: "OpenAI".to_string(),
            kind: "openai-compatible".to_string(),
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4o-mini".to_string(),
            is_default: true,
        });

        assert!(output.contains("openai"));
        assert!(output.contains("gpt-4o-mini"));
        assert!(!output.contains("sk-"));
        assert!(!output.contains("api_key"));
    }

    #[test]
    fn provider_upsert_args_reject_invalid_provider_kinds() {
        let args = ProviderUpsertArgs {
            id: "test".to_string(),
            display_name: "Test".to_string(),
            kind: "invalid-kind".to_string(),
            base_url: "https://example.com/v1".to_string(),
            api_key: "sk-test".to_string(),
            model: "test-model".to_string(),
            is_default: false,
        };

        let error =
            LlmProviderRecord::try_from(args).expect_err("invalid provider kind should fail");
        assert!(error.to_string().contains("invalid llm provider kind"));
    }
}
