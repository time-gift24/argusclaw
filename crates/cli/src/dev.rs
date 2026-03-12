#[cfg(feature = "dev")]
pub mod config;

use std::io::{self, Write};
use std::path::Path;

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use clap::{Args, Parser, Subcommand};
use claw::AppContext;
use claw::agents::AgentId;
use claw::approval::ApprovalResponse;
use claw::db::ApprovalRepository;
use claw::db::SqliteWorkflowRepository;
use claw::db::llm::{
    LlmProviderId, LlmProviderKind, LlmProviderRecord, LlmProviderSummary, SecretString,
};
use claw::db::sqlite::SqliteApprovalRepository;
use claw::llm::LlmStreamEvent;
use claw::workflow::{
    JobId, JobRecord, StageId, StageRecord, WorkflowId, WorkflowRecord, WorkflowRepository,
    WorkflowStatus,
};
use futures_util::StreamExt;
#[cfg(feature = "dev")]
use owo_colors::OwoColorize;
#[cfg(feature = "dev")]
static APPROVAL_DEV_MIGRATOR: sqlx::migrate::Migrator = sqlx::migrate!("./src/dev/migrations");

#[derive(Debug, Parser)]
pub struct DevCli {
    #[command(subcommand)]
    pub command: DevCommand,
}

/// 开发与测试命令，用于 LLM 提供商、Agent 和工作流。
#[derive(Debug, Subcommand)]
pub enum DevCommand {
    /// 管理 LLM 提供商配置。
    #[command(subcommand)]
    Provider(ProviderCommand),
    /// 测试 LLM 补全请求。
    #[command(subcommand)]
    Llm(LlmCommand),
    /// 测试 Agent/LLM Turn 执行流程。
    #[command(subcommand)]
    Turn(TurnCommand),
    /// 管理审批请求和响应。
    #[command(subcommand)]
    Approval(ApprovalCommand),
    /// 管理工作流、阶段和任务。
    #[command(subcommand)]
    Workflow(WorkflowCommand),
}

/// Turn 执行测试命令。
#[derive(Debug, Subcommand)]
pub enum TurnCommand {
    /// 测试 Turn 执行（可配置选项）。
    Test {
        /// 使用的提供商 ID（默认为默认提供商）。
        #[arg(long)]
        provider: Option<String>,

        /// 启用的工具 ID（逗号分隔）。
        #[arg(long, value_delimiter = ',')]
        tools: Vec<String>,

        /// Turn 的系统提示词。
        #[arg(long, default_value = "You are a helpful assistant.")]
        system_prompt: String,

        /// 发送的用户消息。
        #[arg(long)]
        message: String,

        /// 启用详细输出（显示所有消息和工具调用）。
        #[arg(short, long)]
        verbose: bool,
    },
}

/// Thread 测试命令，用于多轮对话流程。
#[derive(Debug, Subcommand)]
pub enum ThreadCommand {
    /// 启动交互式多轮对话。
    Chat {
        /// 使用的提供商 ID（默认为默认提供商）。
        #[arg(long)]
        provider: Option<String>,

        /// 启用的工具 ID（逗号分隔）。
        #[arg(long, value_delimiter = ',')]
        tools: Vec<String>,

        /// 对话的系统提示词。
        #[arg(long, default_value = "You are a helpful assistant.")]
        system_prompt: String,

        /// 启用详细输出（显示所有消息和工具调用）。
        #[arg(short, long)]
        verbose: bool,
    },

    /// 运行自动化多轮对话测试。
    Test {
        /// 使用的提供商 ID（默认为默认提供商）。
        #[arg(long)]
        provider: Option<String>,

        /// 启用的工具 ID（逗号分隔）。
        #[arg(long, value_delimiter = ',')]
        tools: Vec<String>,

        /// 对话的系统提示词。
        #[arg(long, default_value = "You are a helpful assistant.")]
        system_prompt: String,

        /// 执行的 Turn 数量。
        #[arg(long, default_value = "3")]
        turns: u32,

        /// 启用详细输出。
        #[arg(short, long)]
        verbose: bool,
    },
}

/// 审批流程测试命令。
#[derive(Debug, Subcommand)]
pub enum ApprovalCommand {
    /// 列出待处理的审批请求。
    List,

    /// 提交请求并持久化到数据库（用于测试）。
    Submit {
        /// 提交请求的 Agent ID。
        #[arg(long, default_value = "cli-test-agent")]
        agent: String,

        /// 请求审批的工具名称。
        #[arg(long, default_value = "shell_exec")]
        tool: String,

        /// 请求的操作摘要。
        #[arg(long, default_value = "Test action")]
        action: String,

        /// 超时时间（秒）。
        #[arg(long, default_value = "60")]
        timeout: u64,
    },

    /// 使用模拟请求测试审批流程。
    Test {
        /// 提交请求的 Agent ID。
        #[arg(long, default_value = "cli-test-agent")]
        agent: String,

        /// 请求审批的工具名称。
        #[arg(long, default_value = "shell_exec")]
        tool: String,

        /// 超时时间（秒）。
        #[arg(long, default_value = "10")]
        timeout: u64,

        /// 自动批准（模拟批准）。
        #[arg(long)]
        approve: bool,

        /// 自动拒绝（模拟拒绝）。
        #[arg(long)]
        deny: bool,
    },

    /// 解决待处理的审批请求。
    Resolve {
        /// 请求 ID（或前缀）。
        #[arg(long)]
        id: String,

        /// 决定：批准或拒绝。
        #[arg(long)]
        approve: bool,
    },

    /// 显示当前审批策略。
    Policy,

    /// 更新审批策略。
    SetPolicy {
        /// 需要审批的工具（逗号分隔）。
        #[arg(long, value_delimiter = ',')]
        tools: Vec<String>,

        /// 自动批准所有（禁用审批）。
        #[arg(long)]
        auto_approve: bool,
    },

    /// 清除持久化的测试请求。
    Clear,
}

/// 工作流执行测试命令。
#[derive(Debug, Subcommand)]
pub enum WorkflowCommand {
    /// 创建新工作流。
    Create {
        /// 工作流名称。
        name: String,
    },

    /// 列出所有工作流。
    List,

    /// 显示工作流详情（包含阶段和任务）。
    Show {
        /// 工作流 ID。
        id: String,
    },

    /// 删除工作流。
    Delete {
        /// 工作流 ID。
        id: String,
    },

    /// 向工作流添加阶段。
    AddStage {
        /// 工作流 ID。
        #[arg(long)]
        workflow: String,
        /// 阶段名称。
        name: String,
        /// 阶段序号（顺序）。
        sequence: i32,
    },

    /// 向阶段添加任务。
    AddJob {
        /// 阶段 ID。
        #[arg(long)]
        stage: String,
        /// Agent ID。
        #[arg(long)]
        agent: String,
        /// 任务名称。
        name: String,
    },

    /// 更新任务状态。
    JobStatus {
        /// 任务 ID。
        #[arg(long)]
        id: String,
        /// 新状态。
        status: String,
    },

    /// 显示工作流状态树。
    Status {
        /// 工作流 ID。
        id: String,
    },
}

/// LLM 提供商管理命令。
#[derive(Debug, Subcommand)]
pub enum ProviderCommand {
    /// 从 TOML 配置文件导入提供商。
    Import {
        /// TOML 配置文件路径。
        #[arg(long)]
        file: String,
    },
    /// 列出所有已配置的提供商。
    List,
    /// 获取指定提供商的详情。
    Get {
        /// 要查询的提供商 ID。
        #[arg(long)]
        id: String,
    },
    /// 创建或更新提供商配置。
    Upsert(ProviderUpsertArgs),
    /// 设置默认提供商。
    SetDefault {
        /// 要设为默认的提供商 ID。
        #[arg(long)]
        id: String,
    },
    /// 获取当前默认提供商。
    GetDefault,
    /// 为提供商设置额外的请求头。
    SetHeader {
        /// 提供商 ID。
        #[arg(long)]
        id: String,
        /// 请求头名称。
        #[arg(long)]
        name: String,
        /// 请求头值。
        #[arg(long)]
        value: String,
    },
    /// 移除提供商的额外请求头。
    RemoveHeader {
        /// 提供商 ID。
        #[arg(long)]
        id: String,
        /// 要移除的请求头名称。
        #[arg(long)]
        name: String,
    },
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

/// LLM 补全测试命令。
#[derive(Debug, Subcommand)]
pub enum LlmCommand {
    /// 向 LLM 提供商发送补全请求。
    Complete {
        /// 使用的提供商 ID（默认为默认提供商）。
        #[arg(long)]
        provider: Option<String>,
        /// 启用流式输出。
        #[arg(long, default_value_t = false)]
        stream: bool,
        /// 发送给 LLM 的提示词。
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
    pub extra_headers: std::collections::HashMap<String, String>,
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
            extra_headers: value.extra_headers,
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
            extra_headers: value.extra_headers,
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
            extra_headers: std::collections::HashMap::new(),
        })
    }
}

pub async fn try_run(ctx: AppContext) -> Result<bool> {
    let Some(first_arg) = std::env::args().nth(1) else {
        return Ok(false);
    };
    if !matches!(
        first_arg.as_str(),
        "provider" | "llm" | "turn" | "approval" | "workflow"
    ) {
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
        DevCommand::Approval(command) => run_approval_command(ctx, command).await,
        DevCommand::Workflow(command) => run_workflow_command(ctx, command).await,
    }
}

pub fn render_provider_output(record: &ProviderDisplayRecord) -> String {
    let headers_str = if record.extra_headers.is_empty() {
        String::new()
    } else {
        let headers: String = record
            .extra_headers
            .iter()
            .map(|(k, v)| format!("  {k}: {v}"))
            .collect::<Vec<_>>()
            .join("\n");
        format!("\nextra_headers:\n{headers}")
    };

    format!(
        "id: {}\ndisplay_name: {}\nkind: {}\nbase_url: {}\nmodel: {}\nis_default: {}{}",
        record.id,
        record.display_name,
        record.kind,
        record.base_url,
        record.model,
        record.is_default,
        headers_str
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

/// Validates that a header name is valid for HTTP headers.
/// Header names must be ASCII and cannot contain spaces, control characters, or delimiters.
fn validate_header_name(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(anyhow!("header name cannot be empty"));
    }

    for ch in name.chars() {
        if !ch.is_ascii() {
            return Err(anyhow!(
                "header name must be ASCII, found non-ASCII character"
            ));
        }
        // HTTP header names cannot contain these characters
        if matches!(ch, '\r' | '\n' | ':' | ' ' | '\t' | '\0'..='\x1f') {
            return Err(anyhow!("header name contains invalid character: {:?}", ch));
        }
    }

    Ok(())
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
        ProviderCommand::SetHeader { id, name, value } => {
            validate_header_name(&name)?;
            let provider_id = LlmProviderId::new(&id);
            let mut record = ctx.get_provider_record(&provider_id).await?;
            record.extra_headers.insert(name.clone(), value);
            ctx.upsert_provider(record).await?;
            println!("Set header `{name}` on provider `{id}`");
        }
        ProviderCommand::RemoveHeader { id, name } => {
            let provider_id = LlmProviderId::new(&id);
            let mut record = ctx.get_provider_record(&provider_id).await?;
            if record.extra_headers.remove(&name).is_some() {
                ctx.upsert_provider(record).await?;
                println!("Removed header `{name}` from provider `{id}`");
            } else {
                println!("Header `{name}` not found on provider `{id}`");
            }
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

// ---------------------------------------------------------------------------
// Workflow SQLite helpers
// ---------------------------------------------------------------------------

fn resolve_workflow_dev_database_url(
    explicit_database_url: Option<&str>,
    cwd: Option<&Path>,
) -> Result<String> {
    if let Some(database_url) = explicit_database_url.filter(|value| !value.trim().is_empty()) {
        return Ok(database_url.to_string());
    }

    let cwd = match cwd {
        Some(path) => path.to_path_buf(),
        None => std::env::current_dir().context("failed to resolve current working directory")?,
    };
    let tmp_dir = cwd.join("tmp");
    std::fs::create_dir_all(&tmp_dir).with_context(|| {
        format!(
            "failed to create dev workflow tmp directory at {}",
            tmp_dir.display()
        )
    })?;

    let db_path = tmp_dir.join("workflow-dev.sqlite");
    Ok(format!("sqlite:{}", db_path.display()))
}

async fn create_dev_workflow_repository() -> Result<(SqliteWorkflowRepository, String)> {
    let env_database_url = std::env::var("WORKFLOW_DATABASE_URL").ok();
    let database_url = resolve_workflow_dev_database_url(env_database_url.as_deref(), None)?;
    let pool = claw::db::sqlite::connect(&database_url)
        .await
        .with_context(|| {
            format!(
                "failed to connect workflow dev database at `{}`",
                database_url
            )
        })?;

    claw::db::sqlite::migrate(&pool).await.with_context(|| {
        format!(
            "failed to run workflow dev migrations for `{}`",
            database_url
        )
    })?;

    Ok((SqliteWorkflowRepository::new(pool), database_url))
}

// ---------------------------------------------------------------------------
// Approval SQLite helpers
// ---------------------------------------------------------------------------

fn resolve_approval_dev_database_url(
    explicit_database_url: Option<&str>,
    cwd: Option<&Path>,
) -> Result<String> {
    if let Some(database_url) = explicit_database_url.filter(|value| !value.trim().is_empty()) {
        return Ok(database_url.to_string());
    }

    let cwd = match cwd {
        Some(path) => path.to_path_buf(),
        None => std::env::current_dir().context("failed to resolve current working directory")?,
    };
    let tmp_dir = cwd.join("tmp");
    std::fs::create_dir_all(&tmp_dir).with_context(|| {
        format!(
            "failed to create dev approval tmp directory at {}",
            tmp_dir.display()
        )
    })?;

    let db_path = tmp_dir.join("approval-dev.sqlite");
    Ok(format!("sqlite:{}", db_path.display()))
}

async fn create_dev_approval_repository() -> Result<(SqliteApprovalRepository, String)> {
    let env_database_url = std::env::var("APPROVAL_DATABASE_URL").ok();
    let database_url = resolve_approval_dev_database_url(env_database_url.as_deref(), None)?;
    let pool = claw::db::sqlite::connect(&database_url)
        .await
        .with_context(|| {
            format!(
                "failed to connect approval dev database at `{}`",
                database_url
            )
        })?;

    APPROVAL_DEV_MIGRATOR.run(&pool).await.with_context(|| {
        format!(
            "failed to run approval dev migrations for `{}`",
            database_url
        )
    })?;

    Ok((SqliteApprovalRepository::new(pool), database_url))
}

/// Run an approval command.
///
/// This function tests the approval module functionality independently.
async fn run_approval_command(_ctx: AppContext, command: ApprovalCommand) -> Result<()> {
    use claw::approval::{ApprovalDecision, ApprovalManager, ApprovalPolicy, ApprovalRequest};
    use claw::protocol::RiskLevel;
    use std::sync::OnceLock;

    /// Simple risk classification for CLI testing.
    /// In production, this comes from the tool's `risk_level()` method via ToolManager.
    fn classify_risk_for_cli(tool_name: &str) -> RiskLevel {
        match tool_name {
            "shell_exec" | "bash" => RiskLevel::Critical,
            "file_write" | "file_delete" => RiskLevel::High,
            "web_fetch" | "browser_navigate" => RiskLevel::Medium,
            _ => RiskLevel::Low,
        }
    }

    // Use a global manager for CLI testing (simplified approach)
    static MANAGER: OnceLock<std::sync::Arc<ApprovalManager>> = OnceLock::new();

    let manager = MANAGER.get_or_init(|| {
        let policy = ApprovalPolicy::default();
        ApprovalManager::new_shared(policy)
    });

    let (repo, database_url) = create_dev_approval_repository().await?;

    match command {
        ApprovalCommand::List => {
            let requests = repo.list_pending().await?;
            if requests.is_empty() {
                println!("No pending approval requests.");
            } else {
                println!("Storage: {database_url}");
                println!("Pending approval requests ({}):", requests.len());
                for req in requests {
                    println!();
                    println!("  ID:            {}", req.id);
                    println!("  Agent:         {}", req.agent_id);
                    println!("  Tool:          {}", req.tool_name);
                    println!("  Action:        {}", req.action);
                    println!("  Risk Level:    {:?}", req.risk_level);
                    println!("  Timeout:       {}s", req.timeout_secs);
                    println!(
                        "  Requested At:  {}",
                        req.requested_at.format("%Y-%m-%d %H:%M:%S UTC")
                    );
                }
            }
        }

        ApprovalCommand::Submit {
            agent,
            tool,
            action,
            timeout,
        } => {
            let risk_level = classify_risk_for_cli(&tool);
            let req = ApprovalRequest::new(
                agent.clone(),
                tool.clone(),
                action.clone(),
                timeout,
                risk_level,
            );

            let request_id = req.id;

            repo.insert_request(&req).await?;

            println!("Request submitted and persisted:");
            println!();
            println!("  ID:            {request_id}");
            println!("  Agent:         {agent}");
            println!("  Tool:          {tool}");
            println!("  Action:        {action}");
            println!("  Risk Level:    {risk_level:?}");
            println!("  Timeout:       {timeout}s");
            println!("  Storage:       {database_url}");
        }

        ApprovalCommand::Test {
            agent,
            tool,
            timeout,
            approve,
            deny,
        } => {
            if approve && deny {
                return Err(anyhow!("Cannot specify both --approve and --deny"));
            }

            let risk_level = classify_risk_for_cli(&tool);
            let req = ApprovalRequest::new(
                agent.clone(),
                tool.clone(),
                format!("Test approval for {tool}"),
                timeout,
                risk_level,
            );

            let request_id = req.id;

            println!("Submitting approval request...");
            println!();
            println!("  Request ID:   {request_id}");
            println!("  Agent:        {agent}");
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
            let decision = if approve {
                ApprovalDecision::Approved
            } else {
                ApprovalDecision::Denied
            };

            // Parse UUID or try prefix matching
            let request_id = if id.len() == 36 {
                id.parse::<uuid::Uuid>()
                    .map_err(|e| anyhow!("Invalid UUID: {e}"))?
            } else {
                // Try prefix matching in database
                let pending = repo.list_pending().await?;
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

            // Remove from database
            let removed = repo.remove_request(request_id).await?;
            if let Some(req) = removed {
                // Insert response record
                let response = ApprovalResponse {
                    request_id,
                    decision,
                    decided_at: Utc::now(),
                    decided_by: Some("cli-user".to_string()),
                };
                if let Err(err) = repo.insert_response(&response).await {
                    eprintln!(
                        "Warning: failed to persist approval response for {}: {}",
                        request_id, err
                    );
                }
                println!("Request {} -> {:?}", req.id, decision);
            } else {
                // Try in-memory manager as fallback
                match manager.resolve(request_id, decision, Some("cli-user".to_string())) {
                    Ok(response) => {
                        println!("Request {} {:?}", response.request_id, response.decision);
                    }
                    Err(e) => {
                        return Err(anyhow!("{e}"));
                    }
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

        ApprovalCommand::Clear => {
            let count = repo.clear_pending().await?;
            if count > 0 {
                println!(
                    "Cleared {} pending requests from database ({database_url}).",
                    count
                );
            } else {
                println!("No pending requests to clear in {database_url}.");
            }
        }
    }

    Ok(())
}

/// Format workflow status with Unicode symbol and color.
#[cfg(feature = "dev")]
fn format_workflow_status(status: WorkflowStatus) -> String {
    match status {
        WorkflowStatus::Pending => format!("{} {}", "○".yellow(), status.as_str().yellow()),
        WorkflowStatus::Running => format!("{} {}", "⟳".cyan(), status.as_str().cyan()),
        WorkflowStatus::Succeeded => format!("{} {}", "✓".green(), status.as_str().green()),
        WorkflowStatus::Failed => format!("{} {}", "✗".red(), status.as_str().red()),
        WorkflowStatus::Cancelled => format!("{} {}", "⊘".dimmed(), status.as_str().dimmed()),
    }
}

/// Run a workflow command.
///
/// This function tests the workflow module functionality independently.
async fn run_workflow_command(_ctx: AppContext, command: WorkflowCommand) -> Result<()> {
    let (repo, database_url) = create_dev_workflow_repository().await?;

    match command {
        WorkflowCommand::Create { name } => {
            let id = WorkflowId::new(uuid::Uuid::new_v4().to_string());
            let workflow = WorkflowRecord {
                id: id.clone(),
                name: name.clone(),
                status: WorkflowStatus::Pending,
            };

            repo.create_workflow(&workflow).await?;

            println!("Workflow created:");
            println!();
            println!("  ID:     {}", id);
            println!("  Name:   {}", name);
            println!("  Status: {}", workflow.status);
            println!("  Storage: {}", database_url);
        }

        WorkflowCommand::List => {
            let workflows = repo.list_workflows().await?;
            if workflows.is_empty() {
                println!("No workflows found.");
            } else {
                println!("Workflows ({}):", workflows.len());
                println!();
                for wf in workflows {
                    println!("  {} ({})", wf.id, wf.name);
                    println!("    Status: {}", wf.status);
                    println!();
                }
            }
        }

        WorkflowCommand::Show { id } => {
            let workflow_id = WorkflowId::new(id.clone());
            let workflow = repo.get_workflow(&workflow_id).await?;

            match workflow {
                Some(wf) => {
                    println!("Workflow:");
                    println!();
                    println!("  ID:     {}", wf.id);
                    println!("  Name:   {}", wf.name);
                    println!("  Status: {}", wf.status);
                    println!();

                    // Show stages
                    let stages = repo.list_stages_by_workflow(&wf.id).await?;
                    if !stages.is_empty() {
                        println!("  Stages:");
                        for stage in stages {
                            println!("    [{}] {} ({})", stage.sequence, stage.id, stage.name);
                            println!("      Status: {}", stage.status);

                            // Show jobs for this stage
                            let jobs = repo.list_jobs_by_stage(&stage.id).await?;
                            if !jobs.is_empty() {
                                println!("      Jobs:");
                                for job in jobs {
                                    println!(
                                        "        - {} (Agent: {}, Status: {})",
                                        job.name, job.agent_id, job.status
                                    );
                                }
                            }
                            println!();
                        }
                    }
                }
                None => {
                    return Err(anyhow!("Workflow not found: {}", id));
                }
            }
        }

        WorkflowCommand::Delete { id } => {
            let workflow_id = WorkflowId::new(id.clone());
            let deleted = repo.delete_workflow(&workflow_id).await?;

            if deleted {
                println!("Workflow {} deleted.", id);
            } else {
                return Err(anyhow!("Workflow not found: {}", id));
            }
        }

        WorkflowCommand::AddStage {
            workflow,
            name,
            sequence,
        } => {
            let workflow_id = WorkflowId::new(workflow.clone());
            let wf_record = repo.get_workflow(&workflow_id).await?;

            match wf_record {
                Some(_) => {
                    let stage_id = StageId::new(uuid::Uuid::new_v4().to_string());
                    let stage = StageRecord {
                        id: stage_id.clone(),
                        workflow_id,
                        name: name.clone(),
                        sequence,
                        status: WorkflowStatus::Pending,
                    };

                    repo.create_stage(&stage).await?;

                    println!("Stage added:");
                    println!();
                    println!("  ID:        {}", stage_id);
                    println!("  Workflow:  {}", workflow);
                    println!("  Name:      {}", name);
                    println!("  Sequence:  {}", sequence);
                }
                None => {
                    return Err(anyhow!("Workflow not found: {}", workflow));
                }
            }
        }

        WorkflowCommand::AddJob { stage, agent, name } => {
            let stage_id = StageId::new(stage.clone());
            let agent_id = AgentId::new(agent.clone());

            // Verify stage exists by trying to list jobs (empty list is ok)
            let _jobs = repo.list_jobs_by_stage(&stage_id).await?;

            let job_id = JobId::new(uuid::Uuid::new_v4().to_string());
            let job = JobRecord {
                id: job_id.clone(),
                stage_id: stage_id.clone(),
                agent_id,
                name: name.clone(),
                status: WorkflowStatus::Pending,
                started_at: None,
                finished_at: None,
            };

            repo.create_job(&job).await?;

            println!("Job added:");
            println!();
            println!("  ID:      {}", job_id);
            println!("  Stage:   {}", stage);
            println!("  Agent:   {}", agent);
            println!("  Name:    {}", name);
        }

        WorkflowCommand::JobStatus { id, status } => {
            let job_id = JobId::new(id.clone());
            let new_status =
                WorkflowStatus::parse_str(&status).map_err(|e| anyhow!("Invalid status: {}", e))?;

            repo.update_job_status(&job_id, new_status, None, None)
                .await?;

            println!("Job {} status updated to {}", id, new_status);
        }

        WorkflowCommand::Status { id } => {
            let workflow_id = WorkflowId::new(id.clone());
            let workflow = repo.get_workflow(&workflow_id).await?;

            match workflow {
                Some(wf) => {
                    // Print workflow name and status: "name (status)"
                    println!("{} ({})", wf.name, format_workflow_status(wf.status));

                    let stages = repo.list_stages_by_workflow(&wf.id).await?;
                    if stages.is_empty() {
                        println!("  (no stages)");
                    } else {
                        for (stage_idx, stage) in stages.iter().enumerate() {
                            let is_last_stage = stage_idx == stages.len() - 1;

                            // Stage line: use ├─ for non-last, └─ for last
                            let stage_branch = if is_last_stage { "└─ " } else { "├─ " };
                            println!(
                                "{}{} ({})",
                                stage_branch,
                                stage.name,
                                format_workflow_status(stage.status)
                            );

                            // Jobs under this stage
                            let jobs = repo.list_jobs_by_stage(&stage.id).await?;
                            for (job_idx, job) in jobs.iter().enumerate() {
                                let is_last_job = job_idx == jobs.len() - 1;

                                // Prefix: │  for non-last stages,    for last stage
                                let stage_prefix = if is_last_stage { "   " } else { "│  " };

                                // Branch: ├─ for non-last jobs, └─ for last job
                                let job_branch = if is_last_job { "└─ " } else { "├─ " };

                                println!(
                                    "{}{}{} ({}) {}",
                                    stage_prefix,
                                    job_branch,
                                    job.name,
                                    job.agent_id.as_ref().dimmed(),
                                    format_workflow_status(job.status)
                                );
                            }
                        }
                    }

                    // Print done line
                    println!("└─ done ({})", format_workflow_status(wf.status));
                }
                None => {
                    return Err(anyhow!("Workflow not found: {}", id));
                }
            }
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

    use super::{
        ApprovalCommand, DevCli, DevCommand, LlmCommand, ProviderCommand, TurnCommand,
        WorkflowCommand, resolve_approval_dev_database_url, resolve_workflow_dev_database_url,
    };
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
            extra_headers: std::collections::HashMap::new(),
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

    #[test]
    fn parses_approval_submit_command_with_agent() {
        let cli = DevCli::parse_from([
            "cli",
            "approval",
            "submit",
            "--agent",
            "agent-42",
            "--tool",
            "shell_exec",
            "--action",
            "run dangerous command",
            "--timeout",
            "120",
        ]);

        match cli.command {
            DevCommand::Approval(ApprovalCommand::Submit {
                agent,
                tool,
                action,
                timeout,
            }) => {
                assert_eq!(agent, "agent-42");
                assert_eq!(tool, "shell_exec");
                assert_eq!(action, "run dangerous command");
                assert_eq!(timeout, 120);
            }
            _ => panic!("approval submit command should parse"),
        }
    }

    #[test]
    fn parses_approval_test_command_with_agent_and_auto_approve() {
        let cli = DevCli::parse_from([
            "cli",
            "approval",
            "test",
            "--agent",
            "agent-99",
            "--tool",
            "file_write",
            "--timeout",
            "15",
            "--approve",
        ]);

        match cli.command {
            DevCommand::Approval(ApprovalCommand::Test {
                agent,
                tool,
                timeout,
                approve,
                deny,
            }) => {
                assert_eq!(agent, "agent-99");
                assert_eq!(tool, "file_write");
                assert_eq!(timeout, 15);
                assert!(approve);
                assert!(!deny);
            }
            _ => panic!("approval test command should parse"),
        }
    }

    #[test]
    fn approval_database_url_prefers_explicit_override() {
        let resolved = resolve_approval_dev_database_url(Some("sqlite:./custom.db"), None)
            .expect("db url should resolve");
        assert_eq!(resolved, "sqlite:./custom.db");
    }

    #[test]
    fn approval_database_url_defaults_to_tmp_under_current_directory() {
        let run_dir =
            std::env::temp_dir().join(format!("argusclaw-dev-cli-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&run_dir).expect("should create run dir");

        let resolved = resolve_approval_dev_database_url(None, Some(&run_dir))
            .expect("default db url should resolve");

        assert!(
            resolved.ends_with("tmp/approval-dev.sqlite"),
            "resolved path should point to ./tmp/approval-dev.sqlite, got {resolved}"
        );
        assert!(
            run_dir.join("tmp").exists(),
            "tmp directory should be created under run dir"
        );

        std::fs::remove_dir_all(run_dir).expect("should remove run dir");
    }

    #[test]
    fn parses_workflow_create_command() {
        let cli = DevCli::parse_from(["cli", "workflow", "create", "my-workflow"]);

        match cli.command {
            DevCommand::Workflow(WorkflowCommand::Create { name }) => {
                assert_eq!(name, "my-workflow");
            }
            _ => panic!("workflow create command should parse"),
        }
    }

    #[test]
    fn parses_workflow_add_stage_command() {
        let cli = DevCli::parse_from([
            "cli",
            "workflow",
            "add-stage",
            "--workflow",
            "wf-123",
            "stage-1",
            "10",
        ]);

        match cli.command {
            DevCommand::Workflow(WorkflowCommand::AddStage {
                workflow,
                name,
                sequence,
            }) => {
                assert_eq!(workflow, "wf-123");
                assert_eq!(name, "stage-1");
                assert_eq!(sequence, 10);
            }
            _ => panic!("workflow add-stage command should parse"),
        }
    }

    #[test]
    fn parses_workflow_add_job_command() {
        let cli = DevCli::parse_from([
            "cli",
            "workflow",
            "add-job",
            "--stage",
            "stage-456",
            "--agent",
            "agent-789",
            "job-1",
        ]);

        match cli.command {
            DevCommand::Workflow(WorkflowCommand::AddJob { stage, agent, name }) => {
                assert_eq!(stage, "stage-456");
                assert_eq!(agent, "agent-789");
                assert_eq!(name, "job-1");
            }
            _ => panic!("workflow add-job command should parse"),
        }
    }

    #[test]
    fn parses_workflow_status_command() {
        let cli = DevCli::parse_from(["cli", "workflow", "status", "wf-abc"]);

        match cli.command {
            DevCommand::Workflow(WorkflowCommand::Status { id }) => {
                assert_eq!(id, "wf-abc");
            }
            _ => panic!("workflow status command should parse"),
        }
    }

    #[test]
    fn parses_workflow_job_status_command() {
        let cli = DevCli::parse_from([
            "cli",
            "workflow",
            "job-status",
            "--id",
            "job-xyz",
            "in_progress",
        ]);

        match cli.command {
            DevCommand::Workflow(WorkflowCommand::JobStatus { id, status }) => {
                assert_eq!(id, "job-xyz");
                assert_eq!(status, "in_progress");
            }
            _ => panic!("workflow job-status command should parse"),
        }
    }

    #[test]
    fn workflow_database_url_prefers_explicit_override() {
        let resolved = resolve_workflow_dev_database_url(Some("sqlite:./custom-workflow.db"), None)
            .expect("db url should resolve");
        assert_eq!(resolved, "sqlite:./custom-workflow.db");
    }
}
