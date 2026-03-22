use argus_wing::ArgusWing;
use subscription::ThreadSubscriptions;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

mod commands;
mod events;
mod subscription;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize tracing subscriber — logs go to stderr, controlled by RUST_LOG env var
    // e.g., RUST_LOG=arguswing=debug,argus-turn=debug,argus=debug
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(fmt::layer().with_target(true))
        .with(filter)
        .init();

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
            // Account commands
            commands::get_current_user,
            commands::has_any_user,
            commands::setup_account,
            commands::login,
            commands::logout,
            // Credential commands
            commands::list_credentials,
            commands::get_credential,
            commands::add_credential,
            commands::update_credential,
            commands::delete_credential,
            // MCP Server commands
            commands::list_mcp_servers,
            commands::get_mcp_server,
            commands::upsert_mcp_server,
            commands::delete_mcp_server,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
