use argus_wing::ArgusWing;
use subscription::ThreadSubscriptions;

mod commands;
mod events;
mod subscription;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    let wing = rt.block_on(ArgusWing::init(None)).expect("初始化失败");

    // Register default tools
    wing.register_default_tools();

    let subscriptions = ThreadSubscriptions::new();

    tauri::Builder::default()
        .manage(wing)
        .manage(subscriptions)
        .invoke_handler(tauri::generate_handler![
            commands::list_providers,
            commands::get_provider,
            commands::upsert_provider,
            commands::delete_provider,
            commands::set_default_provider,
            commands::test_provider_connection,
            commands::test_provider_input,
            commands::list_agent_templates,
            commands::get_agent_template,
            commands::upsert_agent_template,
            commands::delete_agent_template,
            commands::create_chat_session,
            commands::send_message,
            commands::get_thread_snapshot,
            commands::resolve_approval,
            commands::list_tools,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
