// crates/claw/src/api/types.rs

#[derive(Clone, async_graphql::SimpleObject)]
pub struct Workflow {
    pub id: String,
    pub name: String,
    pub status: String,
    pub jobs: Vec<Job>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Clone, async_graphql::SimpleObject)]
pub struct Job {
    pub id: String,
    pub job_type: String,
    pub name: String,
    pub status: String,
    pub agent_id: String,
    pub context: Option<String>,
    pub prompt: String,
    pub thread_id: Option<String>,
    pub group_id: Option<String>,
    pub depends_on: Vec<String>,
    pub cron_expr: Option<String>,
    pub started_at: Option<String>,
    pub finished_at: Option<String>,
}
