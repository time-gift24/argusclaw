// crates/claw/src/api/mod.rs
pub mod mutation;
pub mod query;
pub mod types;

use crate::workflow::WorkflowRepository;
use async_graphql::Schema;
use mutation::MutationRoot;
use query::QueryRoot;

pub type WorkflowSchema = Schema<QueryRoot, MutationRoot, async_graphql::EmptySubscription>;

pub fn create_schema(repo: Box<dyn WorkflowRepository>) -> WorkflowSchema {
    Schema::build(QueryRoot, MutationRoot, async_graphql::EmptySubscription)
        .data(repo)
        .finish()
}
