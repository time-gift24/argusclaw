// crates/claw/src/api/query.rs
use super::types::{Job, Stage, Workflow};
use crate::workflow::{WorkflowId, WorkflowRepository};
use async_graphql::{Context, ID, Object};

pub struct QueryRoot;

#[Object]
impl QueryRoot {
    async fn workflow(&self, ctx: &Context<'_>, id: ID) -> async_graphql::Result<Option<Workflow>> {
        let repo = ctx.data::<Box<dyn WorkflowRepository>>()?;
        let workflow_id = WorkflowId::new(id.to_string());

        let record = repo.get_workflow(&workflow_id).await?;
        let Some(wf) = record else { return Ok(None) };

        let stages = repo.list_stages_by_workflow(&workflow_id).await?;
        let mut workflow_stages = Vec::new();

        for stage in stages {
            let jobs = repo.list_jobs_by_stage(&stage.id).await?;
            workflow_stages.push(Stage {
                id: stage.id.to_string(),
                name: stage.name,
                sequence: stage.sequence,
                status: stage.status.to_string(),
                jobs: jobs
                    .into_iter()
                    .map(|j| Job {
                        id: j.id.to_string(),
                        name: j.name,
                        status: j.status.to_string(),
                        agent_id: Some(j.agent_id.to_string()),
                        started_at: j.started_at,
                        finished_at: j.finished_at,
                    })
                    .collect(),
            });
        }

        Ok(Some(Workflow {
            id: wf.id.to_string(),
            name: wf.name,
            status: wf.status.to_string(),
            stages: workflow_stages,
            created_at: String::new(),
            updated_at: String::new(),
        }))
    }

    async fn workflows(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<Workflow>> {
        let repo = ctx.data::<Box<dyn WorkflowRepository>>()?;
        let records = repo.list_workflows().await?;

        let mut workflows = Vec::new();
        for wf in records {
            let stages = repo.list_stages_by_workflow(&wf.id).await?;
            let mut workflow_stages = Vec::new();

            for stage in stages {
                let jobs = repo.list_jobs_by_stage(&stage.id).await?;
                workflow_stages.push(Stage {
                    id: stage.id.to_string(),
                    name: stage.name,
                    sequence: stage.sequence,
                    status: stage.status.to_string(),
                    jobs: jobs
                        .into_iter()
                        .map(|j| Job {
                            id: j.id.to_string(),
                            name: j.name,
                            status: j.status.to_string(),
                            agent_id: Some(j.agent_id.to_string()),
                            started_at: j.started_at,
                            finished_at: j.finished_at,
                        })
                        .collect(),
                });
            }

            workflows.push(Workflow {
                id: wf.id.to_string(),
                name: wf.name,
                status: wf.status.to_string(),
                stages: workflow_stages,
                created_at: String::new(),
                updated_at: String::new(),
            });
        }

        Ok(workflows)
    }
}
