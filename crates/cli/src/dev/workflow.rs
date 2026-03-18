//! Workflow command - development only.

use std::path::Path;

use anyhow::{Context, Result, anyhow};
use clap::Subcommand;
use claw::AppContext;
use claw::{AgentId, JobRecord, JobRepository, JobType, SqliteWorkflowRepository};
use claw::{JobId, WorkflowId, WorkflowRecord, WorkflowRepository, WorkflowStatus};
use owo_colors::OwoColorize;

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
    /// 显示工作流详情（包含任务）。
    Show {
        /// 工作流 ID。
        id: String,
    },
    /// 删除工作流。
    Delete {
        /// 工作流 ID。
        id: String,
    },
    /// 向工作流添加任务。
    AddJob {
        /// 工作流 ID (作为 group_id)。
        #[arg(long)]
        workflow: String,
        /// Agent ID。
        #[arg(long)]
        agent: String,
        /// 任务名称。
        name: String,
        /// 任务描述/提示词。
        #[arg(long)]
        prompt: String,
        /// 上下文（可选）。
        #[arg(long)]
        context: Option<String>,
        /// 依赖的其他任务 ID（可选，多个用逗号分隔）。
        #[arg(long)]
        depends_on: Option<String>,
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

/// Resolve workflow dev database URL.
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

/// Create dev workflow repository.
async fn create_dev_workflow_repositories()
-> Result<(SqliteWorkflowRepository, Box<dyn JobRepository>, String)> {
    let env_database_url = std::env::var("WORKFLOW_DATABASE_URL").ok();
    let database_url = resolve_workflow_dev_database_url(env_database_url.as_deref(), None)?;
    let pool = claw::sqlite::connect(&database_url)
        .await
        .with_context(|| {
            format!(
                "failed to connect workflow dev database at `{}`",
                database_url
            )
        })?;

    claw::sqlite::migrate(&pool).await.with_context(|| {
        format!(
            "failed to run workflow dev migrations for `{}`",
            database_url
        )
    })?;

    let workflow_repo = SqliteWorkflowRepository::new(pool.clone());
    let job_repo: Box<dyn JobRepository> = Box::new(claw::SqliteJobRepository::new(pool));

    Ok((workflow_repo, job_repo, database_url))
}

/// Format workflow status with Unicode symbol and color.
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
pub async fn run_workflow_command(_ctx: AppContext, command: WorkflowCommand) -> Result<()> {
    let (workflow_repo, job_repo, database_url) = create_dev_workflow_repositories().await?;

    match command {
        WorkflowCommand::Create { name } => {
            let id = WorkflowId::new(uuid::Uuid::new_v4().to_string());
            let workflow = WorkflowRecord {
                id: id.clone(),
                name: name.clone(),
                status: WorkflowStatus::Pending,
            };

            workflow_repo.create_workflow(&workflow).await?;

            println!("Workflow created:");
            println!();
            println!("  ID:     {}", id);
            println!("  Name:   {}", name);
            println!("  Status: {}", workflow.status);
            println!("  Storage: {}", database_url);
        }

        WorkflowCommand::List => {
            let workflows = workflow_repo.list_workflows().await?;
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
            let workflow = workflow_repo.get_workflow(&workflow_id).await?;

            match workflow {
                Some(wf) => {
                    println!("Workflow:");
                    println!();
                    println!("  ID:     {}", wf.id);
                    println!("  Name:   {}", wf.name);
                    println!("  Status: {}", wf.status);
                    println!();

                    // Show jobs for this workflow (group)
                    let jobs = job_repo.list_by_group(&id).await?;
                    if !jobs.is_empty() {
                        println!("  Jobs:");
                        for job in jobs {
                            println!(
                                "    - {} (Agent: {}, Status: {})",
                                job.name, job.agent_id, job.status
                            );
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
            let deleted = workflow_repo.delete_workflow(&workflow_id).await?;

            if deleted {
                println!("Workflow {} deleted.", id);
            } else {
                return Err(anyhow!("Workflow not found: {}", id));
            }
        }

        WorkflowCommand::AddJob {
            workflow,
            agent,
            name,
            prompt,
            context,
            depends_on,
        } => {
            let workflow_id = WorkflowId::new(workflow.clone());
            // Verify workflow exists
            let _wf = workflow_repo
                .get_workflow(&workflow_id)
                .await?
                .ok_or_else(|| anyhow!("Workflow not found: {}", workflow))?;

            let job_id = JobId::new(uuid::Uuid::new_v4().to_string());
            let agent_id: i64 = agent
                .parse()
                .map_err(|e| anyhow!("Invalid agent id: {}", e))?;
            let agent_id = AgentId::new(agent_id);

            // Parse depends_on
            let depends_on_ids: Vec<JobId> = depends_on
                .map(|s| s.split(',').map(|id| JobId::new(id.trim())).collect())
                .unwrap_or_default();

            let job = JobRecord {
                id: job_id.clone(),
                job_type: JobType::Workflow,
                name: name.clone(),
                status: WorkflowStatus::Pending,
                agent_id,
                context,
                prompt: prompt.clone(),
                thread_id: None,
                group_id: Some(workflow.clone()),
                depends_on: depends_on_ids,
                cron_expr: None,
                scheduled_at: None,
                started_at: None,
                finished_at: None,
            };

            job_repo.create(&job).await?;

            println!("Job added:");
            println!();
            println!("  ID:      {}", job_id);
            println!("  Workflow: {}", workflow);
            println!("  Agent:   {}", agent);
            println!("  Name:    {}", name);
            println!("  Prompt:  {}", prompt);
        }

        WorkflowCommand::JobStatus { id, status } => {
            let job_id = JobId::new(id.clone());
            let new_status =
                WorkflowStatus::parse_str(&status).map_err(|e| anyhow!("Invalid status: {}", e))?;

            job_repo
                .update_status(&job_id, new_status, None, None)
                .await?;

            println!("Job {} status updated to {}", id, new_status);
        }

        WorkflowCommand::Status { id } => {
            let workflow_id = WorkflowId::new(id.clone());
            let workflow = workflow_repo.get_workflow(&workflow_id).await?;

            match workflow {
                Some(wf) => {
                    // Print workflow name and status: "name (status)"
                    println!("{} ({})", wf.name, format_workflow_status(wf.status));

                    let jobs = job_repo.list_by_group(&id).await?;
                    if jobs.is_empty() {
                        println!("  (no jobs)");
                    } else {
                        for (job_idx, job) in jobs.iter().enumerate() {
                            let is_last_job = job_idx == jobs.len() - 1;
                            let job_branch = if is_last_job { "└─ " } else { "├─ " };

                            // Show dependency indicator if any
                            let dep_str = if job.depends_on.is_empty() {
                                String::new()
                            } else {
                                let dep_ids: Vec<&str> =
                                    job.depends_on.iter().map(AsRef::as_ref).collect();
                                format!(" (deps: {})", dep_ids.join(", "))
                            };

                            println!(
                                "  {} ({}){} {}",
                                job_branch,
                                job.name,
                                dep_str,
                                format_workflow_status(job.status)
                            );
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

#[cfg(test)]
mod tests {
    use super::WorkflowCommand;
    use crate::dev::DevCli;
    use crate::dev::DevCommand;
    use clap::Parser;

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
    fn parses_workflow_add_job_command() {
        let cli = DevCli::parse_from([
            "cli",
            "workflow",
            "add-job",
            "--workflow",
            "wf-123",
            "--agent",
            "agent-456",
            "--prompt",
            "do something",
            "my-job",
        ]);

        match cli.command {
            DevCommand::Workflow(WorkflowCommand::AddJob {
                workflow,
                agent,
                name,
                prompt,
                context,
                depends_on,
            }) => {
                assert_eq!(workflow, "wf-123");
                assert_eq!(agent, "agent-456");
                assert_eq!(name, "my-job");
                assert_eq!(prompt, "do something");
                assert!(context.is_none());
                assert!(depends_on.is_none());
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
            "succeeded",
        ]);

        match cli.command {
            DevCommand::Workflow(WorkflowCommand::JobStatus { id, status }) => {
                assert_eq!(id, "job-xyz");
                assert_eq!(status, "succeeded");
            }
            _ => panic!("workflow job-status command should parse"),
        }
    }

    #[test]
    fn workflow_database_url_prefers_explicit_override() {
        let resolved =
            super::resolve_workflow_dev_database_url(Some("sqlite:./custom-workflow.db"), None)
                .expect("db url should resolve");
        assert_eq!(resolved, "sqlite:./custom-workflow.db");
    }
}
