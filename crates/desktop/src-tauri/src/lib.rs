use std::sync::Arc;

use tauri::Manager;

mod commands;
mod tauri_context;

use commands::{
    create_thread, get_default_thread_id, get_thread_messages, send_message, subscribe_thread,
};
use tauri_context::TauriContext;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize tracing for debug logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("desktop=debug".parse().unwrap())
                .add_directive("claw=debug".parse().unwrap())
                .add_directive("argusclaw=debug".parse().unwrap()),
        )
        .init();

    tracing::info!("Starting Tauri application...");

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    let init_result = rt
        .block_on(claw::AppContext::init_with_defaults(None))
        .expect("初始化失败");

    tracing::info!(
        "AppContext initialized with agent_runtime_id: {}, thread_id: {}",
        init_result.agent_runtime_id,
        init_result.thread_id
    );

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(move |app| {
            // Create TauriContext with pre-initialized IDs
            let tauri_ctx = Arc::new(TauriContext::new(
                Arc::new(init_result.context),
                app.handle().clone(),
                init_result.agent_runtime_id,
                init_result.thread_id,
            ));

            // Manage the TauriContext
            app.manage(tauri_ctx);

            tracing::info!("Tauri setup complete");

            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_default_thread_id,
            subscribe_thread,
            send_message,
            get_thread_messages,
            create_thread,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
