use std::sync::Arc;

use claw::AppContext;
use subscription::ThreadSubscriptions;

mod commands;
mod events;
mod subscription;

/// Register default tools with the tool manager.
fn register_default_tools(tool_manager: &Arc<claw::ToolManager>) {
    tool_manager.register(Arc::new(claw::ShellTool::new()));
    tool_manager.register(Arc::new(claw::ReadTool::new()));
    tool_manager.register(Arc::new(claw::GrepTool::new()));
    tool_manager.register(Arc::new(claw::GlobTool::new()));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    let claw_context = rt.block_on(AppContext::init(None)).expect("初始化失败");

    // Register default tools
    register_default_tools(&claw_context.tool_manager());

    let subscriptions = ThreadSubscriptions::new();

    tauri::Builder::default()
        .manage(Arc::new(claw_context))
        .manage(subscriptions)
        .invoke_handler(tauri::generate_handler![
            commands::list_providers,
            commands::get_provider,
            commands::upsert_provider,
            commands::delete_provider,
            commands::set_default_provider,
            commands::test_provider_connection,
            commands::test_provider_input,
            commands::list_models_by_provider,
            commands::upsert_model,
            commands::delete_model,
            commands::set_default_model,
            commands::list_builtin_tools,
            commands::list_agent_templates,
            commands::get_agent_template,
            commands::upsert_agent_template,
            commands::delete_agent_template,
            commands::get_default_agent_template,
            commands::create_default_agent,
            commands::get_current_user,
            commands::has_any_user,
            commands::setup_account,
            commands::login,
            commands::logout,
            commands::create_chat_session,
            commands::send_message,
            commands::get_thread_snapshot,
            commands::resolve_approval,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use claw::ToolManager;

    use super::register_default_tools;

    #[test]
    fn register_default_tools_registers_expected_ids() {
        let tool_manager = Arc::new(ToolManager::new());
        register_default_tools(&tool_manager);

        let ids = tool_manager.list_ids();
        assert!(ids.contains(&"shell".to_string()));
        assert!(ids.contains(&"read".to_string()));
        assert!(ids.contains(&"grep".to_string()));
        assert!(ids.contains(&"glob".to_string()));
    }
}
