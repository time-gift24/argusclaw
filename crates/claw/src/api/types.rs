// crates/claw/src/api/types.rs

#[derive(Clone, async_graphql::SimpleObject)]
pub struct Workflow {
    pub id: String,
    pub name: String,
    pub status: String,
    pub stages: Vec<Stage>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, async_graphql::SimpleObject)]
pub struct Stage {
    pub id: String,
    pub name: String,
    pub sequence: i32,
    pub status: String,
    pub jobs: Vec<Job>,
}

#[derive(Clone, async_graphql::SimpleObject)]
pub struct Job {
    pub id: String,
    pub name: String,
    pub status: String,
    pub agent_id: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}
