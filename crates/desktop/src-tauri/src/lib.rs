use std::path::PathBuf;

use argus_wing::ArgusWing;
use subscription::ThreadSubscriptions;
use tracing_subscriber::{fmt, prelude::*, EnvFilter};

mod commands;
mod events;
mod subscription;

fn default_tracing_directive(is_debug_build: bool) -> &'static str {
    if is_debug_build { "trace" } else { "info" }
}

fn resolve_tracing_filter(rust_log: Option<&str>, is_debug_build: bool) -> EnvFilter {
    match rust_log {
        Some(value) => EnvFilter::new(value),
        None => EnvFilter::new(default_tracing_directive(is_debug_build)),
    }
}

fn default_log_path() -> PathBuf {
    PathBuf::from("./tmp/arguswing.log")
}

fn init_desktop_tracing() {
    let log_path = default_log_path();
    let log_dir = log_path.parent().expect("log path should have a parent");
    std::fs::create_dir_all(log_dir).expect("failed to create log directory");

    let rust_log = std::env::var("RUST_LOG").ok();
    let filter = resolve_tracing_filter(rust_log.as_deref(), cfg!(debug_assertions));
    let file_appender = tracing_appender::rolling::never(log_dir, "arguswing.log");

    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_target(true))
        .with(
            fmt::layer()
                .with_target(true)
                .with_ansi(false)
                .with_writer(file_appender),
        )
        .try_init()
        .expect("failed to initialize desktop tracing");
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    init_desktop_tracing();

    let rt = tokio::runtime::Runtime::new().expect("failed to create tokio runtime");
    let wing = rt.block_on(ArgusWing::init(None)).expect("初始化失败");
    rt.block_on(wing.register_default_tools())
        .expect("默认工具注册失败");

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
            commands::list_subagents,
            commands::add_subagent,
            commands::remove_subagent,
            commands::create_chat_session,
            commands::activate_existing_thread,
            commands::update_thread_model,
            commands::send_message,
            commands::get_thread_snapshot,
            commands::resolve_approval,
            commands::list_sessions,
            commands::delete_session,
            commands::rename_session,
            commands::list_threads,
            commands::rename_thread,
            commands::list_tools,
            // Account commands
            commands::get_current_user,
            commands::has_any_user,
            commands::setup_account,
            commands::login,
            commands::logout,
            commands::get_provider_context_window,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::{default_log_path, default_tracing_directive, resolve_tracing_filter};

    #[test]
    fn default_tracing_directive_uses_trace_for_debug_builds() {
        assert_eq!(default_tracing_directive(true), "trace");
    }

    #[test]
    fn default_tracing_directive_uses_info_for_release_builds() {
        assert_eq!(default_tracing_directive(false), "info");
    }

    #[test]
    fn resolve_tracing_filter_prefers_explicit_rust_log() {
        let filter = resolve_tracing_filter(Some("arguswing=debug,argus=trace"), true);
        let rendered = filter.to_string();

        assert!(rendered.contains("arguswing=debug"));
        assert!(rendered.contains("argus=trace"));
    }

    #[test]
    fn resolve_tracing_filter_falls_back_to_build_default() {
        let filter = resolve_tracing_filter(None, true);
        assert_eq!(filter.to_string(), "trace");
    }

    #[test]
    fn default_log_path_points_to_arguswing_log_file() {
        assert_eq!(default_log_path(), PathBuf::from("./tmp/arguswing.log"));
    }
}
