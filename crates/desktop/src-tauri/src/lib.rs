use std::sync::Arc;

use claw::AppContext;
use tauri::Manager;

mod commands;
mod tauri_context;

use commands::{create_thread, get_thread_messages, send_message, subscribe_thread};
use tauri_context::TauriContext;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    let claw_context = rt.block_on(AppContext::init(None)).expect("初始化失败");

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            // Create TauriContext wrapping both AppContext and AppHandle
            let tauri_ctx = Arc::new(TauriContext::new(
                Arc::new(claw_context),
                app.handle().clone(),
            ));

            // Manage the TauriContext
            app.manage(tauri_ctx);

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            subscribe_thread,
            send_message,
            get_thread_messages,
            create_thread,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
