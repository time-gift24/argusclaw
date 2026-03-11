use claw::{AppContext, Workflow};
use tauri::State;

#[tauri::command]
pub async fn get_workflow(id: String, ctx: State<'_, AppContext>) -> Result<Workflow, String> {
    ctx.workflow_manager()
        .get(&id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn save_workflow(workflow: Workflow, ctx: State<'_, AppContext>) -> Result<(), String> {
    ctx.workflow_manager()
        .save(&workflow)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_workflows(ctx: State<'_, AppContext>) -> Result<Vec<Workflow>, String> {
    ctx.workflow_manager()
        .list()
        .await
        .map_err(|e| e.to_string())
}
