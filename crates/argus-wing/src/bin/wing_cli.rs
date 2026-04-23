use anyhow::{bail, Context, Result};
use argus_protocol::AgentId;
use argus_wing::{ArgusWing, OneShotAgentSelector, OneShotRunRequest};
use clap::{Args, Parser, Subcommand};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(
    name = "argus-wing-cli",
    version,
    about = "Inspect database-backed agents and run a one-shot task with one of them"
)]
struct Cli {
    /// Optional database path (defaults to ~/.arguswing/sqlite.db).
    #[arg(long, global = true)]
    database: Option<String>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// List saved agent templates from the database.
    ListAgents(ListAgentsArgs),
    /// Run a one-shot task with a saved database agent.
    Run(RunArgs),
}

#[derive(Debug, Args)]
struct ListAgentsArgs {
    /// Emit agent rows as JSON instead of a text table.
    #[arg(long, default_value_t = false)]
    json: bool,
}

#[derive(Debug, Args)]
struct RunArgs {
    /// Database agent template id.
    #[arg(long)]
    agent_id: Option<i64>,
    /// Saved agent template display name.
    #[arg(long)]
    agent: Option<String>,
    /// Override the template's system prompt for this one run only.
    #[arg(long)]
    system_prompt: Option<String>,
    /// Override the model for this one run only.
    #[arg(long)]
    model: Option<String>,
    /// Emit the run result as JSON instead of human-readable text.
    #[arg(long, default_value_t = false)]
    json: bool,
    /// The task prompt to send to the selected agent.
    #[arg(long)]
    prompt: String,
}

fn resolve_agent_selector(args: &RunArgs) -> Result<OneShotAgentSelector> {
    match (args.agent_id, args.agent.as_ref()) {
        (Some(agent_id), None) => Ok(OneShotAgentSelector::Id(AgentId::new(agent_id))),
        (None, Some(agent_name)) => Ok(OneShotAgentSelector::DisplayName(agent_name.clone())),
        (None, None) => bail!("select a database agent with --agent-id or --agent"),
        (Some(_), Some(_)) => bail!("use either --agent-id or --agent, not both"),
    }
}

#[derive(Debug, Serialize)]
struct AgentListRow {
    agent_id: i64,
    display_name: String,
    description: String,
    tool_names: Vec<String>,
    provider_id: Option<i64>,
    model_id: Option<String>,
}

fn summarize_text(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let trimmed = text.trim();
    let char_count = trimmed.chars().count();
    if char_count <= max_chars {
        return trimmed.to_string();
    }

    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }

    let truncated: String = trimmed.chars().take(max_chars - 3).collect();
    format!("{truncated}...")
}

fn summarize_tools(tool_names: &[String]) -> String {
    match tool_names {
        [] => "-".to_string(),
        [only] => only.clone(),
        [first, second] => format!("{first}, {second}"),
        [first, second, third] => format!("{first}, {second}, {third}"),
        [first, second, third, rest @ ..] => {
            format!("{first}, {second}, {third} +{} more", rest.len())
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        Commands::ListAgents(args) => {
            let wing = ArgusWing::init(cli.database.as_deref())
                .await
                .context("failed to initialize ArgusWing")?;
            let templates = wing
                .list_templates()
                .await
                .context("failed to list database agent templates")?;

            let rows: Vec<AgentListRow> = templates
                .into_iter()
                .map(|template| AgentListRow {
                    agent_id: template.id.inner(),
                    display_name: template.display_name,
                    description: template.description,
                    tool_names: template.tool_names,
                    provider_id: template.provider_id.map(|provider_id| provider_id.inner()),
                    model_id: template.model_id,
                })
                .collect();

            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&rows)
                        .context("failed to serialize agent list as JSON")?
                );
            } else {
                println!(
                    "{:<8} {:<28} {:<12} {:<20} {:<32} {}",
                    "ID", "Name", "Provider", "Model", "Tools", "Description"
                );
                for row in rows {
                    println!(
                        "{:<8} {:<28} {:<12} {:<20} {:<32} {}",
                        row.agent_id,
                        summarize_text(&row.display_name, 28),
                        row.provider_id
                            .map(|provider_id| provider_id.to_string())
                            .unwrap_or_else(|| "-".to_string()),
                        row.model_id
                            .as_deref()
                            .map(|model_id| summarize_text(model_id, 20))
                            .unwrap_or_else(|| "-".to_string()),
                        summarize_text(&summarize_tools(&row.tool_names), 32),
                        summarize_text(&row.description, 64)
                    );
                }
            }
        }
        Commands::Run(args) => {
            let agent = resolve_agent_selector(&args)?;
            let wing = ArgusWing::init(cli.database.as_deref())
                .await
                .context("failed to initialize ArgusWing")?;
            let result = wing
                .run_one_shot(OneShotRunRequest {
                    agent,
                    prompt: args.prompt,
                    system_prompt: args.system_prompt,
                    model: args.model,
                })
                .await
                .context("one-shot execution failed")?;

            if args.json {
                println!(
                    "{}",
                    serde_json::to_string_pretty(&result)
                        .context("failed to serialize one-shot result as JSON")?
                );
            } else {
                println!("Agent: {} ({})", result.agent_display_name, result.agent_id);
                println!("Model: {}", result.provider_model);
                println!();
                println!("{}", result.assistant_message);
                println!();
                println!(
                    "Token usage: input={}, output={}, total={}",
                    result.token_usage.input_tokens,
                    result.token_usage.output_tokens,
                    result.token_usage.total_tokens
                );
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_agent_selector_rejects_missing_selector() {
        let error = resolve_agent_selector(&RunArgs {
            agent_id: None,
            agent: None,
            system_prompt: None,
            model: None,
            json: false,
            prompt: "run this".to_string(),
        })
        .expect_err("selector resolution should require a database agent choice");

        assert!(error.to_string().contains("--agent-id or --agent"));
    }

    #[test]
    fn resolve_agent_selector_rejects_conflicting_selectors() {
        let error = resolve_agent_selector(&RunArgs {
            agent_id: Some(7),
            agent: Some("Chrome Explore".to_string()),
            system_prompt: None,
            model: None,
            json: false,
            prompt: "run this".to_string(),
        })
        .expect_err("selector resolution should reject conflicting selectors");

        assert!(error.to_string().contains("either --agent-id or --agent"));
    }

    #[test]
    fn resolve_agent_selector_returns_database_agent_id_selector() {
        let selector = resolve_agent_selector(&RunArgs {
            agent_id: Some(7),
            agent: None,
            system_prompt: None,
            model: None,
            json: false,
            prompt: "run this".to_string(),
        })
        .expect("selector resolution should return the chosen database agent");

        assert!(matches!(selector, OneShotAgentSelector::Id(id) if id == AgentId::new(7)));
    }

    #[test]
    fn summarize_text_keeps_short_values_and_truncates_long_ones() {
        assert_eq!(summarize_text("short", 10), "short");
        assert_eq!(summarize_text("this is longer than ten", 10), "this is...");
    }

    #[test]
    fn summarize_tools_handles_empty_and_overflow_lists() {
        assert_eq!(summarize_tools(&[]), "-");
        assert_eq!(
            summarize_tools(&[
                "shell".to_string(),
                "read".to_string(),
                "grep".to_string(),
                "http".to_string(),
            ]),
            "shell, read, grep +1 more"
        );
    }
}
