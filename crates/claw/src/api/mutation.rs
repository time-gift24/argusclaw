// crates/claw/src/api/mutation.rs
use async_graphql::{Context, ID, InputObject, Object};
use uuid::Uuid;

use super::types::{Job, Workflow};
use crate::job::{JobRecord, JobRepository, JobType};
use crate::workflow::{WorkflowId, WorkflowRecord, WorkflowRepository, WorkflowStatus};

#[derive(InputObject)]
pub struct CreateWorkflowInput {
    pub name: String,
}

#[derive(InputObject)]
pub struct AddJobInput {
    pub group_id: Option<String>,
    pub agent_id: String,
    pub name: String,
    pub prompt: String,
    pub context: Option<String>,
    pub job_type: Option<String>,
    pub depends_on: Option<Vec<String>>,
    pub cron_expr: Option<String>,
}

#[derive(InputObject)]
pub struct UpdateJobStatusInput {
    pub job_id: ID,
    pub status: String,
}

pub struct MutationRoot;

#[Object]
impl MutationRoot {
    async fn create_workflow(
        &self,
        ctx: &Context<'_>,
        input: CreateWorkflowInput,
    ) -> async_graphql::Result<Workflow> {
        let workflow_repo = ctx.data::<Box<dyn WorkflowRepository>>()?;
        let id = WorkflowId::new(Uuid::new_v4().to_string());
        let record = WorkflowRecord {
            id: id.clone(),
            name: input.name,
            status: WorkflowStatus::Pending,
        };

        workflow_repo.create_workflow(&record).await?;

        Ok(Workflow {
            id: id.to_string(),
            name: record.name,
            status: record.status.to_string(),
            jobs: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        })
    }

    async fn add_job(&self, ctx: &Context<'_>, input: AddJobInput) -> async_graphql::Result<Job> {
        let job_repo = ctx.data::<Box<dyn JobRepository>>()?;
        let id = crate::workflow::JobId::new(Uuid::new_v4().to_string());
        let agent_id = crate::agents::AgentId::new(input.agent_id.clone());

        let job_type = input
            .job_type
            .as_deref()
            .map(JobType::parse_str)
            .transpose()
            .map_err(async_graphql::Error::new)?;

        let depends_on: Vec<crate::workflow::JobId> = input
            .depends_on
            .unwrap_or_default()
            .into_iter()
            .map(crate::workflow::JobId::new)
            .collect();

        let record = JobRecord {
            id,
            job_type: job_type.unwrap_or(JobType::Standalone),
            name: input.name,
            status: WorkflowStatus::Pending,
            agent_id,
            context: input.context,
            prompt: input.prompt,
            thread_id: None,
            group_id: input.group_id,
            depends_on,
            cron_expr: input.cron_expr,
            scheduled_at: None,
            started_at: None,
            finished_at: None,
        };

        job_repo.create(&record).await?;

        Ok(Job {
            id: record.id.to_string(),
            job_type: record.job_type.to_string(),
            name: record.name,
            status: record.status.to_string(),
            agent_id: input.agent_id,
            context: record.context,
            prompt: record.prompt,
            thread_id: None,
            group_id: record.group_id,
            depends_on: record.depends_on.iter().map(|j| j.to_string()).collect(),
            cron_expr: record.cron_expr,
            started_at: None,
            finished_at: None,
        })
    }

    async fn update_job_status(
        &self,
        ctx: &Context<'_>,
        input: UpdateJobStatusInput,
    ) -> async_graphql::Result<Job> {
        let job_repo = ctx.data::<Box<dyn JobRepository>>()?;
        let job_id = crate::workflow::JobId::new(input.job_id.to_string());
        let status = WorkflowStatus::parse_str(&input.status).map_err(async_graphql::Error::new)?;

        job_repo.update_status(&job_id, status, None, None).await?;

        Ok(Job {
            id: input.job_id.to_string(),
            job_type: String::new(),
            name: String::new(),
            status: status.to_string(),
            agent_id: String::new(),
            context: None,
            prompt: String::new(),
            thread_id: None,
            group_id: None,
            depends_on: Vec::new(),
            cron_expr: None,
            started_at: None,
            finished_at: None,
        })
    }
}
