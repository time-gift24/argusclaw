//! Workflow command - development only.

use std::path::Path;

use anyhow::{Context, Result, anyhow};
use clap::Subcommand;
use claw::AppContext;
use claw::agents::AgentId;
use claw::db::SqliteWorkflowRepository;
use claw::workflow::{
    JobId, JobRecord, StageId, StageRecord, WorkflowId, WorkflowRecord, WorkflowRepository,
    WorkflowStatus,
};
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
///
/// This function tests the workflow module functionality independently.
pub async fn run_workflow_command(_ctx: AppContext, command: WorkflowCommand) -> Result<()> {
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
        let resolved =
            super::resolve_workflow_dev_database_url(Some("sqlite:./custom-workflow.db"), None)
                .expect("db url should resolve");
        assert_eq!(resolved, "sqlite:./custom-workflow.db");
    }
}
