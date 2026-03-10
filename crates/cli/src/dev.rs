#[cfg(feature = "dev")]
pub mod config;

use std::io::{self, Write};
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use clap::{Args, Parser, Subcommand};
use claw::AppContext;
use claw::db::llm::{
    LlmProviderId, LlmProviderKind, LlmProviderRecord, LlmProviderSummary, SecretString,
};
use claw::llm::LlmStreamEvent;
use futures_util::StreamExt;

#[derive(Debug, Parser)]
pub struct DevCli {
    #[command(subcommand)]
    pub command: DevCommand,
}

#[derive(Debug, Subcommand)]
pub enum DevCommand {
    #[command(subcommand)]
    Provider(ProviderCommand),
    #[command(subcommand)]
    Llm(LlmCommand),
}

#[derive(Debug, Subcommand)]
pub enum ProviderCommand {
    Import {
        #[arg(long)]
        file: String,
    },
    List,
    Get {
        #[arg(long)]
        id: String,
    },
    Upsert(ProviderUpsertArgs),
    SetDefault {
        #[arg(long)]
        id: String,
    },
    GetDefault,
}

#[derive(Debug, Args)]
pub struct ProviderUpsertArgs {
    #[arg(long)]
    pub id: String,
    #[arg(long = "display-name")]
    pub display_name: String,
    #[arg(long)]
    pub kind: String,
    #[arg(long = "base-url")]
    pub base_url: String,
    #[arg(long = "api-key")]
    pub api_key: String,
    #[arg(long)]
    pub model: String,
    #[arg(long = "default", default_value_t = false)]
    pub is_default: bool,
}

#[derive(Debug, Subcommand)]
pub enum LlmCommand {
    Complete {
        #[arg(long)]
        provider: Option<String>,
        #[arg(long, default_value_t = false)]
        stream: bool,
        prompt: String,
    },
}

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

pub async fn try_run(ctx: AppContext) -> Result<bool> {
    let Some(first_arg) = std::env::args().nth(1) else {
        return Ok(false);
    };
    if !matches!(first_arg.as_str(), "provider" | "llm") {
        return Ok(false);
    }

    let cli = DevCli::parse();
    run(ctx, cli.command).await?;
    Ok(true)
}

pub async fn run(ctx: AppContext, command: DevCommand) -> Result<()> {
    match command {
        DevCommand::Provider(command) => run_provider_command(ctx, command).await,
        DevCommand::Llm(command) => run_llm_command(ctx, command).await,
    }
}

pub fn render_provider_output(record: &ProviderDisplayRecord) -> String {
    format!(
        "id: {}\ndisplay_name: {}\nkind: {}\nbase_url: {}\nmodel: {}\nis_default: {}",
        record.id,
        record.display_name,
        record.kind,
        record.base_url,
        record.model,
        record.is_default
    )
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
struct StreamRenderState {
    reasoning_started: bool,
    summary_started: bool,
}

fn render_stream_event(state: &mut StreamRenderState, event: &LlmStreamEvent) -> Option<String> {
    match event {
        LlmStreamEvent::ReasoningDelta { delta } if !delta.is_empty() => {
            let mut output = String::new();
            if !state.reasoning_started {
                output.push_str("[Reasoning] ");
                state.reasoning_started = true;
            }
            output.push_str(delta);
            Some(output)
        }
        LlmStreamEvent::ContentDelta { delta } if !delta.is_empty() => {
            let mut output = String::new();
            if !state.summary_started {
                if state.reasoning_started {
                    output.push('\n');
                }
                output.push_str("[Summary] ");
                state.summary_started = true;
            }
            output.push_str(delta);
            Some(output)
        }
        _ => None,
    }
}

fn finish_stream_output(state: &StreamRenderState) -> Option<&'static str> {
    (state.reasoning_started || state.summary_started).then_some("\n")
}

#[cfg(test)]
pub fn render_stream_output(events: &[LlmStreamEvent]) -> String {
    let mut state = StreamRenderState::default();
    let mut output = String::new();

    for event in events {
        if let Some(chunk) = render_stream_event(&mut state, event) {
            output.push_str(&chunk);
        }
    }

    if let Some(suffix) = finish_stream_output(&state) {
        output.push_str(suffix);
    }

    output
}

async fn run_provider_command(ctx: AppContext, command: ProviderCommand) -> Result<()> {
    match command {
        ProviderCommand::Import { file } => {
            let contents = std::fs::read_to_string(Path::new(&file))
                .with_context(|| format!("failed to read provider import file `{file}`"))?;
            let config: config::ProviderImportFile =
                toml::from_str(&contents).context("failed to parse provider import toml")?;
            let records = config.into_records().map_err(|e| anyhow!(e.to_string()))?;
            ctx.import_providers(records).await?;
        }
        ProviderCommand::List => {
            for provider in ctx.llm_manager().list_providers().await? {
                println!("{}", render_provider_output(&provider.into()));
                println!();
            }
        }
        ProviderCommand::Get { id } => {
            let provider = ctx.get_provider_record(&LlmProviderId::new(id)).await?;
            println!("{}", render_provider_output(&provider.into()));
        }
        ProviderCommand::Upsert(args) => {
            let record = LlmProviderRecord::try_from(args).map_err(|e| anyhow!(e.to_string()))?;
            ctx.upsert_provider(record).await?;
        }
        ProviderCommand::SetDefault { id } => {
            ctx.set_default_provider(&LlmProviderId::new(id)).await?;
        }
        ProviderCommand::GetDefault => {
            let provider = ctx.get_default_provider_record().await?;
            println!("{}", render_provider_output(&provider.into()));
        }
    }

    Ok(())
}

async fn run_llm_command(ctx: AppContext, command: LlmCommand) -> Result<()> {
    match command {
        LlmCommand::Complete {
            provider,
            stream,
            prompt,
        } => {
            let provider_id = provider.map(LlmProviderId::new);
            if stream {
                let mut events = ctx.stream_text(provider_id.as_ref(), prompt).await?;
                let mut render_state = StreamRenderState::default();
                let mut stdout = io::stdout();

                while let Some(event) = events.next().await {
                    let event = event?;
                    if let Some(chunk) = render_stream_event(&mut render_state, &event) {
                        write!(stdout, "{chunk}").context("failed to write stream output")?;
                        stdout.flush().context("failed to flush stream output")?;
                    }
                }

                if let Some(suffix) = finish_stream_output(&render_state) {
                    write!(stdout, "{suffix}").context("failed to write stream output")?;
                    stdout.flush().context("failed to flush stream output")?;
                }
            } else {
                let content = ctx.complete_text(provider_id.as_ref(), prompt).await?;
                println!("{content}");
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use claw::db::llm::LlmProviderRecord;
    use claw::llm::LlmStreamEvent;

    use super::{DevCli, DevCommand, LlmCommand, ProviderCommand};
    use crate::dev::{
        ProviderDisplayRecord, ProviderUpsertArgs, render_provider_output, render_stream_output,
    };

    #[test]
    fn parses_provider_import_command() {
        let cli = DevCli::parse_from(["cli", "provider", "import", "--file", "./providers.toml"]);

        match cli.command {
            DevCommand::Provider(ProviderCommand::Import { file }) => {
                assert_eq!(file, "./providers.toml");
            }
            _ => panic!("provider import command should parse"),
        }
    }

    #[test]
    fn parses_llm_complete_command_with_provider_selector_and_streaming() {
        let cli = DevCli::parse_from([
            "cli",
            "llm",
            "complete",
            "--provider",
            "openai",
            "--stream",
            "say hello",
        ]);

        match cli.command {
            DevCommand::Llm(LlmCommand::Complete {
                provider,
                stream,
                prompt,
            }) => {
                assert_eq!(provider.as_deref(), Some("openai"));
                assert!(stream);
                assert_eq!(prompt, "say hello");
            }
            _ => panic!("llm complete command should parse"),
        }
    }

    #[test]
    fn parses_llm_complete_command_with_default_provider() {
        let cli = DevCli::parse_from(["cli", "llm", "complete", "say hello"]);

        match cli.command {
            DevCommand::Llm(LlmCommand::Complete {
                provider,
                stream,
                prompt,
            }) => {
                assert_eq!(provider, None);
                assert!(!stream);
                assert_eq!(prompt, "say hello");
            }
            _ => panic!("llm complete command should parse"),
        }
    }

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

        assert!(output.contains("OpenAI"));
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

    #[test]
    fn render_stream_output_formats_reasoning_and_summary_sections() {
        let output = render_stream_output(&[
            LlmStreamEvent::ReasoningDelta {
                delta: "step 1".to_string(),
            },
            LlmStreamEvent::ReasoningDelta {
                delta: " -> step 2".to_string(),
            },
            LlmStreamEvent::ContentDelta {
                delta: "final answer".to_string(),
            },
        ]);

        assert_eq!(
            output,
            "[Reasoning] step 1 -> step 2\n[Summary] final answer\n"
        );
    }
}
