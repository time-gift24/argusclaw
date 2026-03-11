// crates/claw/src/api/query.rs
use async_graphql::{Context, Object, ID};
use super::types::{Workflow, Stage, Job};

pub struct QueryRoot;

#[Object]
impl QueryRoot {
    async fn workflow(&self, ctx: &Context<'_>, id: ID) -> async_graphql::Result<Option<Workflow>> {
        // TODO: get from repository
        todo!()
    }

    async fn workflows(&self, ctx: &Context<'_>) -> async_graphql::Result<Vec<Workflow>> {
        // TODO: list from repository
        todo!()
    }
}
