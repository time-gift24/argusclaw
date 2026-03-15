//! Approval command - development only.

use std::sync::OnceLock;

use anyhow::{Context, Result, anyhow};
use chrono::Utc;
use clap::Subcommand;
use claw::sqlite::SqliteApprovalRepository;
use claw::{
    ApprovalDecision, ApprovalManager, ApprovalPolicy, ApprovalRepository, ApprovalRequest,
    ApprovalResponse, RiskLevel,
};
use uuid::Uuid;

use super::APPROVAL_DEV_MIGRATOR;

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

/// Resolve approval dev database URL.
fn resolve_approval_dev_database_url(
    explicit_database_url: Option<&str>,
    cwd: Option<&std::path::Path>,
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

/// Create dev approval repository.
async fn create_dev_approval_repository() -> Result<(SqliteApprovalRepository, String)> {
    let env_database_url = std::env::var("APPROVAL_DATABASE_URL").ok();
    let database_url = resolve_approval_dev_database_url(env_database_url.as_deref(), None)?;
    let pool = claw::sqlite::connect(&database_url)
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

/// Run approval command.
///
/// This function tests the approval module functionality independently.
pub async fn run_approval_command(command: ApprovalCommand) -> Result<()> {
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
                id.parse::<Uuid>()
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
                    "Cleared {} pending requests from database ({}).",
                    count, database_url
                );
            } else {
                println!("No pending requests to clear in {}.", database_url);
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::ApprovalCommand;
    use crate::dev::DevCli;
    use crate::dev::DevCommand;
    use clap::Parser;

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
        let resolved = super::resolve_approval_dev_database_url(Some("sqlite:./custom.db"), None)
            .expect("db url should resolve");
        assert_eq!(resolved, "sqlite:./custom.db");
    }

    #[test]
    fn approval_database_url_defaults_to_tmp_under_current_directory() {
        let run_dir =
            std::env::temp_dir().join(format!("argusclaw-dev-cli-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&run_dir).expect("should create run dir");

        let resolved = super::resolve_approval_dev_database_url(None, Some(&run_dir))
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
}
