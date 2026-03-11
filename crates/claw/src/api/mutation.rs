// crates/claw/src/api/mutation.rs
use async_graphql::{Context, Object, ID, InputObject};
use super::types::{Workflow, Stage, Job};

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
    async fn create_workflow(&self, ctx: &Context<'_>, input: CreateWorkflowInput) -> async_graphql::Result<Workflow> {
        todo!()
    }

    async fn add_stage(&self, ctx: &Context<'_>, input: AddStageInput) -> async_graphql::Result<Stage> {
        todo!()
    }

    async fn add_job(&self, ctx: &Context<'_>, input: AddJobInput) -> async_graphql::Result<Job> {
        todo!()
    }

    async fn update_job_status(&self, ctx: &Context<'_>, input: UpdateJobStatusInput) -> async_graphql::Result<Job> {
        todo!()
    }
}
