use async_graphql::{Request, Variables};
use claw::api::{create_schema, WorkflowSchema};
use claw::db::sqlite;
use claw::db::SqliteWorkflowRepository;
use std::path::PathBuf;
use std::sync::Arc;
use tauri::{command, State};
use tokio::sync::RwLock;

pub struct AppState {
    pub schema: Arc<RwLock<WorkflowSchema>>,
}

fn expand_database_path(path: &str) -> String {
    if path.starts_with('~') {
        let home = dirs::home_dir().expect("failed to get home directory");
        let path = path.trim_start_matches('~');
        let expanded: PathBuf = home.join(path.trim_start_matches('/'));
        expanded.to_string_lossy().to_string()
    } else {
        path.to_string()
    }
}

fn ensure_parent_dir(path: &str) -> std::io::Result<()> {
    let path = PathBuf::from(path);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(())
}

#[command]
async fn query_workflow(state: State<'_, AppState>, id: String) -> Result<String, String> {
    let guard = state.schema.read().await;
    let query = r#"query($id: ID!) { workflow(id: $id) { id name status stages { id name jobs { id name status } } } } }"#;
    let variables: Variables =
        serde_json::from_str(&format!(r#"{{"id": "{}"}}"#, id)).map_err(|e| e.to_string())?;
    let request = Request::new(query).variables(variables);
    let res = guard.execute(request).await;
    if !res.errors.is_empty() {
        return Err(res
            .errors
            .iter()
            .map(|e| e.message.clone())
            .collect::<Vec<_>>()
            .join(", "));
    }
    Ok(res.data.to_string())
}

#[command]
async fn query_workflows(state: State<'_, AppState>) -> Result<String, String> {
    let guard = state.schema.read().await;
    let query = "{ workflows { id name status } }";
    let res = guard.execute(query).await;
    if !res.errors.is_empty() {
        return Err(res
            .errors
            .iter()
            .map(|e| e.message.clone())
            .collect::<Vec<_>>()
            .join(", "));
    }
    Ok(res.data.to_string())
}

#[command]
async fn create_workflow(state: State<'_, AppState>, name: String) -> Result<String, String> {
    let guard = state.schema.read().await;
    let query =
        r#"mutation($name: String!) { createWorkflow(input: { name: $name }) { id name } }"#;
    let variables: Variables =
        serde_json::from_str(&format!(r#"{{"name": "{}"}}"#, name)).map_err(|e| e.to_string())?;
    let request = Request::new(query).variables(variables);
    let res = guard.execute(request).await;
    if !res.errors.is_empty() {
        return Err(res
            .errors
            .iter()
            .map(|e| e.message.clone())
            .collect::<Vec<_>>()
            .join(", "));
    }
    Ok(res.data.to_string())
}

#[command]
async fn add_stage(
    state: State<'_, AppState>,
    workflow_id: String,
    name: String,
    sequence: i32,
) -> Result<String, String> {
    let guard = state.schema.read().await;
    let query = r#"mutation($workflowId: ID!, $name: String!, $sequence: Int!) { addStage(input: { workflowId: $workflowId, name: $name, sequence: $sequence }) { id name } }"#;
    let variables: Variables = serde_json::from_str(&format!(
        r#"{{"workflowId": "{}", "name": "{}", "sequence": {}}}"#,
        workflow_id, name, sequence
    ))
    .map_err(|e| e.to_string())?;
    let request = Request::new(query).variables(variables);
    let res = guard.execute(request).await;
    if !res.errors.is_empty() {
        return Err(res
            .errors
            .iter()
            .map(|e| e.message.clone())
            .collect::<Vec<_>>()
            .join(", "));
    }
    Ok(res.data.to_string())
}

#[command]
async fn add_job(
    state: State<'_, AppState>,
    stage_id: String,
    agent_id: String,
    name: String,
) -> Result<String, String> {
    let guard = state.schema.read().await;
    let query = r#"mutation($stageId: ID!, $agentId: String!, $name: String!) { addJob(input: { stageId: $stageId, agentId: $agentId, name: $name }) { id name } }"#;
    let variables: Variables = serde_json::from_str(&format!(
        r#"{{"stageId": "{}", "agentId": "{}", "name": "{}"}}"#,
        stage_id, agent_id, name
    ))
    .map_err(|e| e.to_string())?;
    let request = Request::new(query).variables(variables);
    let res = guard.execute(request).await;
    if !res.errors.is_empty() {
        return Err(res
            .errors
            .iter()
            .map(|e| e.message.clone())
            .collect::<Vec<_>>()
            .join(", "));
    }
    Ok(res.data.to_string())
}

#[command]
async fn update_job_status(
    state: State<'_, AppState>,
    job_id: String,
    status: String,
) -> Result<String, String> {
    let guard = state.schema.read().await;
    let query = r#"mutation($jobId: ID!, $status: String!) { updateJobStatus(input: { jobId: $jobId, status: $status }) { id status } }"#;
    let variables: Variables = serde_json::from_str(&format!(
        r#"{{"jobId": "{}", "status": "{}"}}"#,
        job_id, status
    ))
    .map_err(|e| e.to_string())?;
    let request = Request::new(query).variables(variables);
    let res = guard.execute(request).await;
    if !res.errors.is_empty() {
        return Err(res
            .errors
            .iter()
            .map(|e| e.message.clone())
            .collect::<Vec<_>>()
            .join(", "));
    }
    Ok(res.data.to_string())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize schema synchronously at startup
    let schema = tokio::runtime::Handle::current().block_on(async {
        let database_url =
            std::env::var("DATABASE_URL").unwrap_or_else(|_| "~/.argusclaw/sqlite.db".to_string());

        let expanded = expand_database_path(&database_url);
        ensure_parent_dir(&expanded).expect("failed to create parent directory");

        let pool = sqlite::connect(&expanded)
            .await
            .expect("Failed to connect to database");

        // Run migrations
        sqlite::migrate(&pool)
            .await
            .expect("Failed to run migrations");

        let repo = SqliteWorkflowRepository::new(pool);
        let schema = create_schema(Box::new(repo));
        Arc::new(RwLock::new(schema))
    });

    let app_state = AppState { schema };

    tauri::Builder::default()
        .manage(app_state)
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
