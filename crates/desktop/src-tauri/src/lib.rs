use claw::AppContext;

mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let ctx = tauri::async_runtime::block_on(async { AppContext::new_desktop().await });

    tauri::Builder::default()
        .manage(ctx)
        .invoke_handler(tauri::generate_handler![
            commands::workflow::get_workflow,
            commands::workflow::save_workflow,
            commands::workflow::list_workflows,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
