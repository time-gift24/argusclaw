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
#[cfg(feature = "dev")]
use owo_colors::OwoColorize;

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
    #[command(subcommand)]
    Turn(TurnCommand),
    #[command(subcommand)]
    Approval(ApprovalCommand),
}

/// Turn execution commands for testing agent/LLM turn flow.
#[derive(Debug, Subcommand)]
pub enum TurnCommand {
    /// Test turn execution with configurable options.
    Test {
        /// Provider ID to use (defaults to default provider).
        #[arg(long)]
        provider: Option<String>,

        /// Tool IDs to enable (comma-separated).
        #[arg(long, value_delimiter = ',')]
        tools: Vec<String>,

        /// System prompt for the turn.
        #[arg(long, default_value = "You are a helpful assistant.")]
        system_prompt: String,

        /// User message to send.
        #[arg(long)]
        message: String,

        /// Enable verbose output (shows all messages and tool calls).
        #[arg(short, long)]
        verbose: bool,
    },
}

/// Approval commands for testing the approval flow.
#[derive(Debug, Subcommand)]
pub enum ApprovalCommand {
    /// List pending approval requests.
    List,

    /// Test approval flow with a simulated request.
    Test {
        /// Tool name to request approval for.
        #[arg(long, default_value = "shell_exec")]
        tool: String,

        /// Timeout in seconds.
        #[arg(long, default_value = "10")]
        timeout: u64,

        /// Auto-approve (simulate approval).
        #[arg(long)]
        approve: bool,

        /// Auto-deny (simulate denial).
        #[arg(long)]
        deny: bool,
    },

    /// Resolve a pending approval request.
    Resolve {
        /// Request ID (or prefix).
        #[arg(long)]
        id: String,

        /// Decision: approve or deny.
        #[arg(long)]
        approve: bool,
    },

    /// Show current approval policy.
    Policy,

    /// Update approval policy.
    SetPolicy {
        /// Tools requiring approval (comma-separated).
        #[arg(long, value_delimiter = ',')]
        tools: Vec<String>,

        /// Auto-approve all (disables approval).
        #[arg(long)]
        auto_approve: bool,
    },
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
    if !matches!(first_arg.as_str(), "provider" | "llm" | "turn" | "approval") {
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
        DevCommand::Turn(command) => run_turn_command(ctx, command).await,
        DevCommand::Approval(command) => run_approval_command(command).await,
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

/// Run an approval command.
///
/// This function tests the approval module functionality independently.
async fn run_approval_command(command: ApprovalCommand) -> Result<()> {
    use claw::approval::{ApprovalDecision, ApprovalManager, ApprovalPolicy, ApprovalRequest};
    use std::sync::OnceLock;

    // Use a global manager for CLI testing (simplified approach)
    static MANAGER: OnceLock<std::sync::Arc<ApprovalManager>> = OnceLock::new();

    let manager = MANAGER.get_or_init(|| {
        let policy = ApprovalPolicy::default();
        ApprovalManager::new_shared(policy)
    });

    match command {
        ApprovalCommand::List => {
            let pending = manager.list_pending();
            if pending.is_empty() {
                println!("No pending approval requests.");
            } else {
                println!("Pending approval requests ({}):", pending.len());
                for req in pending {
                    println!();
                    println!("  ID:            {}", req.id);
                    println!("  Agent:         {}", req.agent_id);
                    println!("  Tool:          {}", req.tool_name);
                    println!("  Action:        {}", req.action_summary);
                    println!("  Risk Level:    {:?}", req.risk_level);
                    println!("  Timeout:       {}s", req.timeout_secs);
                    println!(
                        "  Requested At:  {}",
                        req.requested_at.format("%Y-%m-%d %H:%M:%S UTC")
                    );
                }
            }
        }

        ApprovalCommand::Test {
            tool,
            timeout,
            approve,
            deny,
        } => {
            if approve && deny {
                return Err(anyhow!("Cannot specify both --approve and --deny"));
            }

            let req = ApprovalRequest::new(
                "cli-test-agent".to_string(),
                tool.clone(),
                format!("Test approval for {tool}"),
                timeout,
            );

            let request_id = req.id;
            let risk_level = ApprovalManager::classify_risk(&tool);

            println!("Submitting approval request...");
            println!();
            println!("  Request ID:   {request_id}");
            println!("  Tool:         {tool}");
            println!("  Risk Level:   {risk_level:?}");
            println!("  Timeout:      {timeout}s");
            println!();

            // If auto-approve or auto-deny, spawn a task to resolve it
            if approve || deny {
                let mgr_clone = std::sync::Arc::clone(manager);
                let decision = if approve {
                    ApprovalDecision::Approved
                } else {
                    ApprovalDecision::Denied
                };
                tokio::spawn(async move {
                    tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                    let _ = mgr_clone.resolve(request_id, decision, Some("cli-auto".to_string()));
                });
            }

            println!("Waiting for resolution...");
            let decision = manager.request_approval(req).await;

            println!();
            match decision {
                ApprovalDecision::Approved => {
                    println!("Result: APPROVED");
                }
                ApprovalDecision::Denied => {
                    println!("Result: DENIED");
                }
                ApprovalDecision::TimedOut => {
                    println!("Result: TIMED OUT");
                }
            }
        }

        ApprovalCommand::Resolve { id, approve } => {
            // Parse UUID (support prefix matching)
            let request_id = if id.len() == 36 {
                id.parse::<uuid::Uuid>()
                    .map_err(|e| anyhow!("Invalid UUID: {e}"))?
            } else {
                // Try prefix matching
                let pending = manager.list_pending();
                let matching: Vec<_> = pending
                    .iter()
                    .filter(|r| r.id.to_string().starts_with(&id))
                    .collect();

                match matching.len() {
                    0 => return Err(anyhow!("No pending request found with ID prefix: {id}")),
                    1 => matching[0].id,
                    _ => {
                        return Err(anyhow!(
                            "Ambiguous ID prefix '{}'. Found {} matching requests.",
                            id,
                            matching.len()
                        ));
                    }
                }
            };

            let decision = if approve {
                ApprovalDecision::Approved
            } else {
                ApprovalDecision::Denied
            };

            match manager.resolve(request_id, decision, Some("cli-user".to_string())) {
                Ok(response) => {
                    println!("Request {} {:?}", response.request_id, response.decision);
                }
                Err(e) => {
                    return Err(anyhow!("{e}"));
                }
            }
        }

        ApprovalCommand::Policy => {
            let policy = manager.policy();
            println!("Current Approval Policy:");
            println!();
            println!("  Require Approval: {:?}", policy.require_approval);
            println!("  Timeout:          {}s", policy.timeout_secs);
            println!("  Auto-approve:     {}", policy.auto_approve);
        }

        ApprovalCommand::SetPolicy {
            tools,
            auto_approve,
        } => {
            let new_policy = if auto_approve {
                ApprovalPolicy {
                    require_approval: vec![],
                    timeout_secs: 60,
                    auto_approve_autonomous: false,
                    auto_approve: true,
                }
            } else {
                ApprovalPolicy {
                    require_approval: tools,
                    timeout_secs: 60,
                    auto_approve_autonomous: false,
                    auto_approve: false,
                }
            };

            new_policy
                .validate()
                .map_err(|e| anyhow!("Invalid policy: {e}"))?;

            manager.update_policy(new_policy);

            println!("Policy updated.");
            println!();
            let policy = manager.policy();
            println!("  Require Approval: {:?}", policy.require_approval);
            println!("  Timeout:          {}s", policy.timeout_secs);
        }
    }

    Ok(())
}

/// Run a turn execution command.
///
/// This function exercises the full turn execution flow including
/// optional tool integration and hook support.
#[cfg(feature = "dev")]
async fn run_turn_command(ctx: AppContext, command: TurnCommand) -> Result<()> {
    use claw::agents::turn::{TurnConfig, TurnInputBuilder, execute_turn};
    use claw::llm::ChatMessage;

    let TurnCommand::Test {
        provider,
        tools,
        system_prompt,
        message,
        verbose,
    } = command;

    // Get provider from context via LLM manager
    let provider = if let Some(id) = provider {
        ctx.llm_manager()
            .get_provider(&LlmProviderId::new(id))
            .await?
    } else {
        ctx.llm_manager().get_default_provider().await?
    };

    // Build tool manager with requested tools
    let tool_manager = std::sync::Arc::new(claw::tool::ToolManager::new());
    // TODO: Register tools based on IDs when tools are implemented

    // Build turn input
    let input = TurnInputBuilder::default()
        .provider(provider)
        .messages(vec![ChatMessage::user(message)])
        .system_prompt(system_prompt)
        .tool_manager(tool_manager)
        .tool_ids(tools)
        .build();

    // Execute turn with timing
    let start = std::time::Instant::now();
    let output = execute_turn(input, TurnConfig::default()).await?;
    let duration = start.elapsed();

    // Render output
    println!();
    println!("{}", "═".repeat(60).dimmed());
    println!("{}", " Turn Execution Results ".bold().cyan());
    println!("{}", "═".repeat(60).dimmed());
    println!();

    // Message history
    println!("{}", "Messages:".bold());
    for msg in &output.messages {
        let role_str = match msg.role {
            claw::llm::Role::User => "USER",
            claw::llm::Role::Assistant => "ASSISTANT",
            claw::llm::Role::System => "SYSTEM",
            claw::llm::Role::Tool => "TOOL",
        };
        let role_colored = match msg.role {
            claw::llm::Role::User => role_str.blue().to_string(),
            claw::llm::Role::Assistant => role_str.green().to_string(),
            claw::llm::Role::System => role_str.yellow().to_string(),
            claw::llm::Role::Tool => role_str.magenta().to_string(),
        };
        let content = if verbose {
            msg.content.clone()
        } else {
            // Truncate to 100 chars for non-verbose
            if msg.content.len() > 100 {
                format!("{}...", &msg.content[..100])
            } else {
                msg.content.clone()
            }
        };
        println!("  {role_colored} {content}");

        if verbose && let Some(tool_calls) = &msg.tool_calls {
            for tc in tool_calls {
                let args = tc.arguments.to_string();
                println!("    {}({})", tc.name.cyan(), args.dimmed());
            }
        }
    }

    println!();

    // Token usage table
    println!("{}", "Token Usage:".bold());
    println!(
        "  {:<15} {}",
        "Input Tokens".dimmed(),
        output.token_usage.input_tokens
    );
    println!(
        "  {:<15} {}",
        "Output Tokens".dimmed(),
        output.token_usage.output_tokens
    );
    println!(
        "  {:<15} {}",
        "Total Tokens".dimmed(),
        output.token_usage.total_tokens
    );
    println!();

    // Summary
    println!("{}", "Summary:".bold());
    println!("  Duration: {duration:?}");
    println!("  Messages: {}", output.messages.len());

    Ok(())
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use claw::db::llm::LlmProviderRecord;
    use claw::llm::LlmStreamEvent;

    use super::{DevCli, DevCommand, LlmCommand, ProviderCommand, TurnCommand};
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

    #[test]
    fn parses_turn_test_command_with_all_options() {
        let cli = DevCli::parse_from([
            "cli",
            "turn",
            "test",
            "--provider",
            "openai",
            "--tools",
            "echo,http",
            "--system-prompt",
            "Be helpful",
            "--message",
            "Hello!",
            "--verbose",
        ]);

        match cli.command {
            DevCommand::Turn(TurnCommand::Test {
                provider,
                tools,
                system_prompt,
                message,
                verbose,
            }) => {
                assert_eq!(provider, Some("openai".to_string()));
                assert_eq!(tools, vec!["echo".to_string(), "http".to_string()]);
                assert_eq!(system_prompt, "Be helpful");
                assert_eq!(message, "Hello!");
                assert!(verbose);
            }
            _ => panic!("turn test command should parse"),
        }
    }

    #[test]
    fn parses_turn_test_command_with_defaults() {
        let cli = DevCli::parse_from(["cli", "turn", "test", "--message", "Hi"]);

        match cli.command {
            DevCommand::Turn(TurnCommand::Test {
                provider,
                tools,
                system_prompt,
                message,
                verbose,
            }) => {
                assert_eq!(provider, None);
                assert!(tools.is_empty());
                assert_eq!(system_prompt, "You are a helpful assistant.");
                assert_eq!(message, "Hi");
                assert!(!verbose);
            }
            _ => panic!("turn test command should parse"),
        }
    }
}
