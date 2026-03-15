use std::sync::Arc;

use claw::AppContext;

mod commands;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    let claw_context = rt.block_on(AppContext::init(None)).expect("初始化失败");

    tauri::Builder::default()
        .manage(Arc::new(claw_context))
        .invoke_handler(tauri::generate_handler![
            commands::list_providers,
            commands::get_provider,
            commands::upsert_provider,
            commands::delete_provider,
            commands::set_default_provider,
            commands::list_agent_templates,
            commands::get_agent_template,
            commands::upsert_agent_template,
            commands::delete_agent_template,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
