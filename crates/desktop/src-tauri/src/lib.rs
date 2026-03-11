use claw::api::{create_schema, WorkflowSchema};
use claw::db::SqliteWorkflowRepository;
use claw::db::sqlite;
use tauri::command;
use std::sync::Arc;
use tokio::sync::RwLock;

// Global schema state
static SCHEMA: once_cell::sync::OnceCell<Arc<RwLock<WorkflowSchema>>> = once_cell::sync::OnceCell::new();

fn get_schema() -> Arc<RwLock<WorkflowSchema>> {
    SCHEMA.get_or_init(|| {
        let database_url = std::env::var("DATABASE_URL")
            .unwrap_or_else(|_| "~/.argusclaw/sqlite.db".to_string());

        let pool = tokio::runtime::Handle::current()
            .block_on(sqlite::connect(&database_url))
            .expect("Failed to connect to database");
        let repo = SqliteWorkflowRepository::new(pool);
        let schema = create_schema(Box::new(repo));
        Arc::new(RwLock::new(schema))
    }).clone()
}

#[command]
async fn query_workflow(id: String) -> Result<String, String> {
    let schema = get_schema();
    let guard = schema.read().await;
    let query = format!(r#"{{ workflow(id: "{}") {{ id name status stages {{ id name jobs {{ id name status }} }} }} }}"#, id);
    let res = guard.execute(&query).await;
    Ok(res.data.to_string())
}

#[command]
async fn query_workflows() -> Result<String, String> {
    let schema = get_schema();
    let guard = schema.read().await;
    let query = "{ workflows { id name status } }";
    let res = guard.execute(query).await;
    Ok(res.data.to_string())
}

#[command]
async fn create_workflow(name: String) -> Result<String, String> {
    let schema = get_schema();
    let guard = schema.read().await;
    let query = format!(r#"{{ createWorkflow(input: {{ name: "{}" }}) {{ id name }} }}"#, name);
    let res = guard.execute(&query).await;
    Ok(res.data.to_string())
}

#[command]
async fn add_stage(workflow_id: String, name: String, sequence: i32) -> Result<String, String> {
    let schema = get_schema();
    let guard = schema.read().await;
    let query = format!(r#"{{ addStage(input: {{ workflowId: "{}", name: "{}", sequence: {} }}) {{ id name }} }}"#, workflow_id, name, sequence);
    let res = guard.execute(&query).await;
    Ok(res.data.to_string())
}

#[command]
async fn add_job(stage_id: String, agent_id: String, name: String) -> Result<String, String> {
    let schema = get_schema();
    let guard = schema.read().await;
    let query = format!(r#"{{ addJob(input: {{ stageId: "{}", agentId: "{}", name: "{}" }}) {{ id name }} }}"#, stage_id, agent_id, name);
    let res = guard.execute(&query).await;
    Ok(res.data.to_string())
}

#[command]
async fn update_job_status(job_id: String, status: String) -> Result<String, String> {
    let schema = get_schema();
    let guard = schema.read().await;
    let query = format!(r#"{{ updateJobStatus(input: {{ jobId: "{}", status: "{}" }}) {{ id status }} }}"#, job_id, status);
    let res = guard.execute(&query).await;
    Ok(res.data.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![
            query_workflow,
            query_workflows,
            create_workflow,
            add_stage,
            add_job,
            update_job_status,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
