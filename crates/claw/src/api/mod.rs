// crates/claw/src/api/mod.rs
pub mod mutation;
pub mod query;
pub mod types;

use crate::job::JobRepository;
use crate::workflow::WorkflowRepository;
use crate::db::thread::ThreadRepository;
use async_graphql::Schema;
use mutation::MutationRoot;
use query::QueryRoot;

pub type WorkflowSchema = Schema<QueryRoot, MutationRoot, async_graphql::EmptySubscription>;

pub fn create_schema(
    workflow_repo: Box<dyn WorkflowRepository>,
    job_repo: Box<dyn JobRepository>,
    thread_repo: Box<dyn ThreadRepository>,
) -> WorkflowSchema {
    Schema::build(QueryRoot, MutationRoot, async_graphql::EmptySubscription)
        .data(workflow_repo)
        .data(job_repo)
        .data(thread_repo)
        .finish()
}
