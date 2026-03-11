// crates/claw/src/api/mod.rs
pub mod types;
pub mod query;
pub mod mutation;

use async_graphql::Schema;
use query::QueryRoot;
use mutation::MutationRoot;

pub type WorkflowSchema = Schema<QueryRoot, MutationRoot, async_graphql::EmptySubscription>;

pub fn create_schema() -> WorkflowSchema {
    Schema::build(QueryRoot, MutationRoot, async_graphql::EmptySubscription).finish()
}
