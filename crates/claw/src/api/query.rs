// crates/claw/src/api/query.rs
use super::types::{Job, Workflow};
use crate::job::JobRepository;
use crate::workflow::{WorkflowId, WorkflowRepository};
use async_graphql::{Context, ID, Object};

pub struct QueryRoot;

#[Object]
impl QueryRoot {
    async fn workflow(&self, ctx: &Context<'_>, id: ID) -> async_graphql::Result<Option<Workflow>> {
        let workflow_repo = ctx.data::<Box<dyn WorkflowRepository>>()?;
        let job_repo = ctx.data::<Box<dyn JobRepository>>()?;
        let workflow_id = WorkflowId::new(id.to_string());

        let record = workflow_repo.get_workflow(&workflow_id).await?;
        let Some(wf) = record else { return Ok(None) };

        let jobs = job_repo.list_by_group(&id.to_string()).await?;

        Ok(Some(Workflow {
            id: wf.id.to_string(),
            name: wf.name,
            status: wf.status.to_string(),
            jobs: jobs
                .into_iter()
                .map(|j| Job {
                    id: j.id.to_string(),
                    job_type: j.job_type.to_string(),
                    name: j.name,
                    status: j.status.to_string(),
                    agent_id: j.agent_id.to_string(),
                    context: j.context,
                    prompt: j.prompt,
                    thread_id: j.thread_id.map(|t| t.to_string()),
                    group_id: j.group_id,
                    depends_on: j.depends_on.iter().map(|j| j.to_string()).collect(),
                    cron_expr: j.cron_expr,
                    started_at: j.started_at,
                    finished_at: j.finished_at,
                })
                .collect(),
            created_at: String::new(),
            updated_at: String::new(),
        }))
    }

    async fn workflows(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<Workflow>> {
        let workflow_repo = ctx.data::<Box<dyn WorkflowRepository>>()?;
        let job_repo = ctx.data::<Box<dyn JobRepository>>()?;
        let records = workflow_repo.list_workflows().await?;

        let mut workflows = Vec::new();
        for wf in records {
            let jobs = job_repo.list_by_group(wf.id.as_ref()).await?;

            workflows.push(Workflow {
                id: wf.id.to_string(),
                name: wf.name,
                status: wf.status.to_string(),
                jobs: jobs
                    .into_iter()
                    .map(|j| Job {
                        id: j.id.to_string(),
                        job_type: j.job_type.to_string(),
                        name: j.name,
                        status: j.status.to_string(),
                        agent_id: j.agent_id.to_string(),
                        context: j.context,
                        prompt: j.prompt,
                        thread_id: j.thread_id.map(|t| t.to_string()),
                        group_id: j.group_id,
                        depends_on: j.depends_on.iter().map(|j| j.to_string()).collect(),
                        cron_expr: j.cron_expr,
                        started_at: j.started_at,
                        finished_at: j.finished_at,
                    })
                    .collect(),
                created_at: String::new(),
                updated_at: String::new(),
            });
        }

        Ok(workflows)
    }
}
