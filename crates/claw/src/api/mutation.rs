// crates/claw/src/api/mutation.rs
use async_graphql::{Context, ID, InputObject, Object};
use uuid::Uuid;

use super::types::{Job, Stage, Workflow};
use crate::agents::AgentId;
use crate::workflow::{
    JobId, JobRecord, StageId, StageRecord, WorkflowId, WorkflowRecord, WorkflowRepository,
    WorkflowStatus,
};

#[derive(InputObject)]
pub struct CreateWorkflowInput {
    pub name: String,
}

#[derive(InputObject)]
pub struct AddStageInput {
    pub workflow_id: ID,
    pub name: String,
    pub sequence: i32,
}

#[derive(InputObject)]
pub struct AddJobInput {
    pub stage_id: ID,
    pub agent_id: String,
    pub name: String,
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
        let repo = ctx.data::<Box<dyn WorkflowRepository>>()?;
        let id = WorkflowId::new(Uuid::new_v4().to_string());
        let record = WorkflowRecord {
            id: id.clone(),
            name: input.name,
            status: WorkflowStatus::Pending,
        };

        repo.create_workflow(&record).await?;

        Ok(Workflow {
            id: id.to_string(),
            name: record.name,
            status: record.status.to_string(),
            stages: Vec::new(),
            created_at: String::new(),
            updated_at: String::new(),
        })
    }

    async fn add_stage(
        &self,
        ctx: &Context<'_>,
        input: AddStageInput,
    ) -> async_graphql::Result<Stage> {
        let repo = ctx.data::<Box<dyn WorkflowRepository>>()?;
        let id = StageId::new(Uuid::new_v4().to_string());
        let workflow_id = WorkflowId::new(input.workflow_id.to_string());

        let record = StageRecord {
            id: id.clone(),
            workflow_id,
            name: input.name,
            sequence: input.sequence,
            status: WorkflowStatus::Pending,
        };

        repo.create_stage(&record).await?;

        Ok(Stage {
            id: id.to_string(),
            name: record.name,
            sequence: record.sequence,
            status: record.status.to_string(),
            jobs: Vec::new(),
        })
    }

    async fn add_job(&self, ctx: &Context<'_>, input: AddJobInput) -> async_graphql::Result<Job> {
        let repo = ctx.data::<Box<dyn WorkflowRepository>>()?;
        let id = JobId::new(Uuid::new_v4().to_string());
        let stage_id = StageId::new(input.stage_id.to_string());
        let agent_id = AgentId::new(input.agent_id.clone());

        let record = JobRecord {
            id: id.clone(),
            stage_id,
            agent_id,
            name: input.name,
            status: WorkflowStatus::Pending,
            started_at: None,
            finished_at: None,
        };

        repo.create_job(&record).await?;

        Ok(Job {
            id: id.to_string(),
            name: record.name,
            status: record.status.to_string(),
            agent_id: Some(input.agent_id),
            started_at: None,
            finished_at: None,
        })
    }

    async fn update_job_status(
        &self,
        ctx: &Context<'_>,
        input: UpdateJobStatusInput,
    ) -> async_graphql::Result<Job> {
        let repo = ctx.data::<Box<dyn WorkflowRepository>>()?;
        let job_id = JobId::new(input.job_id.to_string());
        let status = WorkflowStatus::parse_str(&input.status).map_err(async_graphql::Error::new)?;

        repo.update_job_status(&job_id, status, None, None).await?;

        Ok(Job {
            id: input.job_id.to_string(),
            name: String::new(),
            status: status.to_string(),
            agent_id: None,
            started_at: None,
            finished_at: None,
        })
    }
}
