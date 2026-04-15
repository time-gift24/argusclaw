mod error;
mod installer;
mod manager;
mod models;
mod patcher;
mod policy;
mod session;
mod tool;

pub use tool::ChromeTool;

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::{Cursor, Write};
    use std::path::{Path, PathBuf};
    use std::sync::Arc;
    use std::sync::Mutex as StdMutex;

    use serde_json::json;
    use tempfile::tempdir;
    use thirtyfour::common::cookie::{Cookie, SameSite};
    use zip::write::SimpleFileOptions;

    use super::error::ChromeToolError;
    use super::installer::{ChromeInstaller, ChromePaths, DriverDownloader};
    use super::manager::{
        BackendOpenResult, BrowserBackend, ChromeHost, DetectedChrome, SessionMode,
    };
    use super::models::{ChromeAction, ChromeToolArgs, PageMetadata};
    use super::patcher::patch_cdc_tokens;
    use super::policy::ExplorePolicy;
    use super::session::BrowserSession;
    use super::tool::ChromeTool;
    use argus_protocol::NamedTool;
    use argus_protocol::ToolExecutionContext;
    use argus_protocol::ids::ThreadId;
    use argus_protocol::tool::ToolError;
    use tokio::sync::broadcast;
    fn make_ctx() -> Arc<ToolExecutionContext> {
        let (pipe_tx, _) = broadcast::channel(16);
        Arc::new(ToolExecutionContext {
            thread_id: ThreadId::new(),
            agent_id: None,
            pipe_tx,
        })
    }

    #[test]
    fn navigate_requires_url() {
        let err = ChromeToolArgs::validate(json!({ "action": "navigate" })).unwrap_err();
        assert!(matches!(
            err,
            ChromeToolError::MissingRequiredField { action, field }
            if action == "navigate" && field == "url"
        ));
    }

    #[test]
    fn navigate_rejects_malformed_url() {
        let err = ChromeToolArgs::validate(json!({ "action": "navigate", "url": "not-a-url" }))
            .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));
    }

    #[test]
    fn navigate_rejects_non_http_scheme() {
        let err =
            ChromeToolArgs::validate(json!({ "action": "navigate", "url": "file:///tmp/a.txt" }))
                .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));

        let err =
            ChromeToolArgs::validate(json!({ "action": "navigate", "url": "chrome://settings" }))
                .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));
    }

    #[test]
    fn navigate_rejects_local_or_private_targets() {
        let err = ChromeToolArgs::validate(json!({
            "action": "navigate",
            "url": "https://localhost/path"
        }))
        .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));

        let err = ChromeToolArgs::validate(json!({
            "action": "navigate",
            "url": "https://127.0.0.1/path"
        }))
        .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));

        let err =
            ChromeToolArgs::validate(json!({ "action": "navigate", "url": "http://10.0.0.1" }))
                .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));

        let err =
            ChromeToolArgs::validate(json!({ "action": "navigate", "url": "http://0.0.0.0" }))
                .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));
    }

    #[test]
    fn navigate_stores_trimmed_validated_url() {
        let args = ChromeToolArgs::validate(
            json!({ "action": "navigate", "url": "  https://example.com/path?q=1  " }),
        )
        .unwrap();

        assert_eq!(args.url.as_deref(), Some("https://example.com/path?q=1"));
    }

    #[test]
    fn navigate_rejects_stray_fields() {
        let err = ChromeToolArgs::validate(json!({
            "action": "navigate",
            "url": "https://example.com",
            "text": "hello",
        }))
        .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));

        let err = ChromeToolArgs::validate(json!({
            "action": "navigate",
            "url": "https://example.com",
            "tab_id": "tab-1",
        }))
        .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));

        let err = ChromeToolArgs::validate(json!({
            "action": "navigate",
            "url": "https://example.com",
            "session_id": "session-1",
        }))
        .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));
    }

    #[test]
    fn click_requires_selector_and_rejects_session_id() {
        let err = ChromeToolArgs::validate(json!({
            "action": "click",
        }))
        .unwrap_err();
        assert!(matches!(
            err,
            ChromeToolError::MissingRequiredField { action, field }
            if action == "click" && field == "selector"
        ));

        let err = ChromeToolArgs::validate(json!({
            "action": "click",
            "selector": "#btn",
            "session_id": "session-1",
        }))
        .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));

        let err = ChromeToolArgs::validate(json!({
            "action": "click",
            "selector": "#btn",
            "url": "https://example.com"
        }))
        .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));
    }

    #[test]
    fn deny_unknown_fields_is_enforced() {
        let err = ChromeToolArgs::validate(json!({ "action": "wait", "unexpected": "value" }))
            .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));
    }

    #[test]
    fn wait_rejects_selector_argument() {
        let err = ChromeToolArgs::validate(json!({
            "action": "wait",
            "selector": "#hero"
        }))
        .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));
    }

    #[test]
    fn install_and_wait_reject_tab_id_argument() {
        let err = ChromeToolArgs::validate(json!({
            "action": "install",
            "tab_id": "tab-1",
        }))
        .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));

        let err = ChromeToolArgs::validate(json!({
            "action": "wait",
            "tab_id": "tab-1",
        }))
        .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));
    }

    #[test]
    fn close_accepts_no_arguments() {
        let args = ChromeToolArgs::validate(json!({
            "action": "close",
        }))
        .unwrap();
        assert_eq!(args.action.as_str(), "close");
    }

    #[test]
    fn close_rejects_unexpected_url() {
        let err = ChromeToolArgs::validate(json!({
            "action": "close",
            "url": "https://example.com",
        }))
        .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));
    }

    #[test]
    fn open_is_not_a_valid_action() {
        let err = ChromeToolArgs::validate(json!({
            "action": "open",
            "url": "https://example.com",
        }))
        .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));
    }

    #[test]
    fn restart_is_not_a_valid_action() {
        let err = ChromeToolArgs::validate(json!({
            "action": "restart",
            "url": "https://example.com",
        }))
        .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));
    }

    #[test]
    fn click_is_rejected_by_policy() {
        let err = ExplorePolicy::readonly()
            .validate_action(ChromeAction::Click)
            .unwrap_err();
        assert!(matches!(
            err,
            ChromeToolError::ActionNotAllowed { action } if action == "click"
        ));
    }

    #[test]
    fn extract_text_is_allowed() {
        ExplorePolicy::readonly()
            .validate_action(ChromeAction::ExtractText)
            .unwrap();
    }

    #[test]
    fn install_is_allowed_by_readonly_policy() {
        ExplorePolicy::readonly()
            .validate_action(ChromeAction::Install)
            .unwrap();
    }

    #[test]
    fn chrome_tool_definition_lists_only_readonly_actions() {
        let tool = ChromeTool::new_for_test(Arc::new(FakeChromeManager));
        let def = tool.definition();
        assert_eq!(def.name, "chrome");
        assert!(def.description.contains("explicit driver install"));
        assert!(def.description.contains("hidden shared browser session"));
        assert!(def.description.contains("navigate(url)"));

        let action_enum = def.parameters["properties"]["action"]["enum"]
            .as_array()
            .expect("action enum should be present");
        let action_values: Vec<&str> = action_enum
            .iter()
            .map(|value| value.as_str().expect("enum value should be a string"))
            .collect();
        assert!(action_values.contains(&"navigate"));
        assert!(action_values.contains(&"wait"));
        assert!(action_values.contains(&"extract_text"));
        assert!(action_values.contains(&"install"));
        assert!(action_values.contains(&"close"));
        assert!(action_values.contains(&"new_tab"));
        assert!(action_values.contains(&"switch_tab"));
        assert!(action_values.contains(&"close_tab"));
        assert!(action_values.contains(&"list_tabs"));
        assert!(!action_values.contains(&"open"));
        assert!(!action_values.contains(&"restart"));
        assert!(!action_values.contains(&"click"));
        assert!(!action_values.contains(&"list_links"));
        assert!(!action_values.contains(&"get_dom_summary"));
        assert!(!action_values.contains(&"network_requests"));

        assert!(def.parameters["properties"].get("session_id").is_none());
        assert!(def.parameters["properties"].get("selector").is_some());
        assert!(def.parameters["properties"].get("max_requests").is_none());
        assert!(def.parameters["properties"].get("text").is_none());
    }

    #[test]
    fn chrome_tool_definition_lists_interactive_actions() {
        let tool = ChromeTool::new_interactive_with_backend(Arc::new(FakeChromeManager));
        let def = tool.definition();
        assert_eq!(def.name, "chrome");
        assert!(def.description.contains("interactive"));
        assert!(def.description.contains("hidden shared browser session"));
        assert!(def.description.contains("navigate(url)"));

        let action_enum = def.parameters["properties"]["action"]["enum"]
            .as_array()
            .expect("action enum should be present");
        let action_values: Vec<&str> = action_enum
            .iter()
            .map(|value| value.as_str().expect("enum value should be a string"))
            .collect();
        assert!(action_values.contains(&"click"));
        assert!(action_values.contains(&"type"));
        assert!(action_values.contains(&"get_url"));
        assert!(action_values.contains(&"get_cookies"));
        assert!(action_values.contains(&"install"));
        assert!(action_values.contains(&"close"));
        assert!(action_values.contains(&"navigate"));
        assert!(action_values.contains(&"new_tab"));
        assert!(action_values.contains(&"switch_tab"));
        assert!(action_values.contains(&"close_tab"));
        assert!(action_values.contains(&"list_tabs"));
        assert!(!action_values.contains(&"open"));
        assert!(!action_values.contains(&"restart"));
        assert!(!action_values.contains(&"list_links"));
        assert!(!action_values.contains(&"get_dom_summary"));
        assert!(!action_values.contains(&"network_requests"));
        assert!(def.parameters["properties"].get("session_id").is_none());
        assert!(def.parameters["properties"].get("text").is_some());
        assert!(def.parameters["properties"].get("max_requests").is_none());
    }

    #[tokio::test]
    async fn chrome_tool_rejects_denied_action_before_backend() {
        let tool = ChromeTool::new_for_test(Arc::new(FakeChromeManager));
        // Click is blocked by readonly policy even with valid args
        let err = tool
            .execute(
                json!({
                    "action": "click",
                    "selector": "#btn",
                }),
                make_ctx(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::NotAuthorized(_)));
    }

    #[tokio::test]
    async fn chrome_tool_navigate_boots_shared_session_without_public_session_id() {
        let tool = ChromeTool::new_for_test(Arc::new(FakeChromeBackend::default().with_page(
            "https://example.com",
            "https://example.com/landing",
            "Example Title",
            Vec::new(),
            "Visible page text",
        )));

        let navigate = tool
            .execute(
                json!({
                    "action": "navigate",
                    "url": "https://example.com"
                }),
                make_ctx(),
            )
            .await
            .expect("navigate should succeed");
        assert_eq!(navigate["action"], "navigate");
        assert!(navigate.get("session_id").is_none());
        assert_eq!(navigate["final_url"], "https://example.com/landing");
    }

    #[tokio::test]
    async fn chrome_tool_dispatches_read_actions_through_hidden_shared_session() {
        let tool = ChromeTool::new_for_test(Arc::new(FakeChromeBackend::default().with_page(
            "https://example.com",
            "https://example.com/landing",
            "Example Title",
            Vec::new(),
            "Visible page text",
        )));

        tool.execute(
            json!({
                "action": "navigate",
                "url": "https://example.com"
            }),
            make_ctx(),
        )
        .await
        .expect("navigate should succeed");

        let wait = tool
            .execute(
                json!({
                    "action": "wait"
                }),
                make_ctx(),
            )
            .await
            .expect("wait should succeed");
        assert_eq!(wait["status"], "ok");

        let extract = tool
            .execute(
                json!({
                    "action": "extract_text",
                    "selector": "main"
                }),
                make_ctx(),
            )
            .await
            .expect("extract_text should succeed");
        assert_eq!(extract["content"], "Visible page text [main]");

        for removed_action in ["list_links", "get_dom_summary", "network_requests"] {
            let err = tool
                .execute(
                    json!({
                        "action": removed_action,
                    }),
                    make_ctx(),
                )
                .await
                .expect_err("removed summary actions should be rejected");
            assert!(matches!(
                err,
                ToolError::ExecutionFailed { reason, .. }
                if reason.contains("unknown variant")
            ));
        }
    }

    #[tokio::test]
    async fn chrome_tool_close_shuts_down_session() {
        let tool = ChromeTool::new_for_test(Arc::new(FakeChromeBackend::default().with_page(
            "https://example.com",
            "https://example.com/landing",
            "Example Title",
            vec![],
            "Visible page text",
        )));

        tool.execute(
            json!({
                "action": "navigate",
                "url": "https://example.com"
            }),
            make_ctx(),
        )
        .await
        .expect("navigate should succeed");

        let close = tool
            .execute(
                json!({
                    "action": "close",
                }),
                make_ctx(),
            )
            .await
            .expect("close should succeed");
        assert_eq!(close["status"], "ok");

        let err = tool
            .execute(
                json!({
                    "action": "extract_text",
                }),
                make_ctx(),
            )
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ToolError::ExecutionFailed { reason, .. }
            if reason.contains("navigate(url)")
        ));
    }

    #[tokio::test]
    async fn chrome_tool_requires_navigate_before_extract_text() {
        let tool = ChromeTool::new_for_test(Arc::new(FakeChromeBackend::default()));
        let err = tool
            .execute(
                json!({
                    "action": "extract_text",
                }),
                make_ctx(),
            )
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ToolError::ExecutionFailed { reason, .. }
            if reason.contains("navigate(url)")
        ));
    }

    #[tokio::test]
    async fn get_cookies_filters_by_domain() {
        let tool = ChromeTool::new_interactive_with_backend(Arc::new(
            FakeChromeBackend::default().with_page_and_cookies(
                "https://www.example.com",
                "https://www.example.com",
                "Example",
                vec![],
                "Visible page text",
                vec![
                    {
                        let mut cookie = Cookie::new("shared", "1");
                        cookie.set_domain(".example.com");
                        cookie.set_path("/");
                        cookie.set_same_site(SameSite::Lax);
                        cookie
                    },
                    {
                        let mut cookie = Cookie::new("host", "2");
                        cookie.set_domain("www.example.com");
                        cookie.set_path("/");
                        cookie.set_secure(true);
                        cookie
                    },
                    {
                        let mut cookie = Cookie::new("api", "3");
                        cookie.set_domain("api.example.com");
                        cookie.set_path("/");
                        cookie
                    },
                    {
                        let mut cookie = Cookie::new("other", "4");
                        cookie.set_domain("other.example.net");
                        cookie.set_path("/");
                        cookie.set_expiry(42);
                        cookie
                    },
                ],
            ),
        ));

        tool.execute(
            json!({
                "action": "navigate",
                "url": "https://www.example.com"
            }),
            make_ctx(),
        )
        .await
        .expect("navigate should succeed");

        let cookies = tool
            .execute(
                json!({
                    "action": "get_cookies",
                    "domain": "www.example.com"
                }),
                make_ctx(),
            )
            .await
            .expect("get_cookies should succeed");

        let names: Vec<&str> = cookies["cookies"]
            .as_array()
            .expect("cookies should be an array")
            .iter()
            .filter_map(|cookie| cookie["name"].as_str())
            .collect();
        assert_eq!(names, vec!["shared", "host"]);
        assert_eq!(cookies["cookies"][0]["sameSite"], "Lax");
        assert_eq!(cookies["cookies"][1]["secure"], true);
    }

    #[tokio::test]
    async fn chrome_tool_install_installs_and_reports_result() {
        let home = tempdir().unwrap();
        let paths = ChromePaths::from_home(home.path());
        let host = Arc::new(FakeManagedChromeHost::new(
            home.path().join("Google Chrome"),
            "124",
            "Managed Example",
        ));
        let downloader = SpyManagedDownloader::with_zip_bytes(fake_driver_zip());
        let tool =
            ChromeTool::new_with_managed_components_for_test(host, downloader, paths.clone());

        let result = tool
            .execute(json!({ "action": "install" }), make_ctx())
            .await
            .unwrap();

        assert_eq!(result["browser_version"], "124");
        assert_eq!(result["driver_version"], "124.0.6367.91");
        assert_eq!(result["cache_hit"], false);
        let driver_path = PathBuf::from(result["driver_path"].as_str().unwrap());
        assert!(driver_path.starts_with(&paths.patched));
        assert!(driver_path.is_file());
    }

    #[tokio::test]
    async fn chrome_tool_install_reuses_cached_driver_without_network() {
        let home = tempdir().unwrap();
        let paths = ChromePaths::from_home(home.path());
        let host = Arc::new(FakeManagedChromeHost::new(
            home.path().join("Google Chrome"),
            "124",
            "Managed Example",
        ));
        ChromeInstaller::new(
            paths.clone(),
            SpyManagedDownloader::with_zip_bytes(fake_driver_zip()),
        )
        .ensure_driver("124")
        .await
        .unwrap();
        let downloader = SpyManagedDownloader::new(HashMap::new());
        let tool =
            ChromeTool::new_with_managed_components_for_test(host, downloader.clone(), paths);

        let result = tool
            .execute(json!({ "action": "install" }), make_ctx())
            .await
            .unwrap();

        assert_eq!(result["cache_hit"], true);
        assert!(downloader.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn managed_constructor_requires_explicit_install_before_navigate() {
        let home = tempdir().unwrap();
        let paths = ChromePaths::from_home(home.path());
        let host = Arc::new(FakeManagedChromeHost::new(
            home.path().join("Google Chrome"),
            "124",
            "Managed Example",
        ));
        let downloader = SpyManagedDownloader::new(HashMap::new());
        let tool = ChromeTool::new_with_managed_components_for_test(
            host.clone(),
            downloader.clone(),
            paths,
        );

        let err = tool
            .execute(
                json!({
                    "action": "navigate",
                    "url": "https://example.com"
                }),
                make_ctx(),
            )
            .await
            .expect_err("managed navigate should require explicit install");

        assert!(matches!(
            err,
            ToolError::ExecutionFailed { tool_name, reason }
                if tool_name == "chrome"
                    && reason.contains("action: install")
                    && reason.contains("not installed")
        ));
        assert!(host.open_calls.lock().unwrap().is_empty());
        assert!(downloader.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn managed_constructor_uses_cached_driver_after_explicit_install() {
        let home = tempdir().unwrap();
        let paths = ChromePaths::from_home(home.path());
        let host = Arc::new(FakeManagedChromeHost::new(
            home.path().join("Google Chrome"),
            "124",
            "Managed Example",
        ));
        let installer = ChromeInstaller::new(
            paths.clone(),
            SpyManagedDownloader::with_zip_bytes(fake_driver_zip()),
        );
        installer.ensure_driver("124").await.unwrap();
        let tool = ChromeTool::new_with_managed_components_for_test(
            host.clone(),
            SpyManagedDownloader::new(HashMap::new()),
            paths.clone(),
        );

        let navigate = tool
            .execute(
                json!({
                    "action": "navigate",
                    "url": "https://example.com"
                }),
                make_ctx(),
            )
            .await
            .expect("managed navigate should succeed with cached driver");

        assert_eq!(navigate["page_title"], "Managed Example");

        let open_call = host
            .open_calls
            .lock()
            .unwrap()
            .last()
            .expect("host should receive an open call")
            .clone();
        assert_eq!(open_call.browser_binary, home.path().join("Google Chrome"));
        assert!(open_call.driver_binary.starts_with(&paths.patched));
        assert!(open_call.driver_binary.is_file());
        assert_eq!(open_call.session_mode, SessionMode::Readonly);
    }

    #[tokio::test]
    async fn interactive_managed_constructor_uses_interactive_session_mode() {
        let home = tempdir().unwrap();
        let paths = ChromePaths::from_home(home.path());
        let host = Arc::new(FakeManagedChromeHost::new(
            home.path().join("Google Chrome"),
            "124",
            "Managed Example",
        ));
        let tool = ChromeTool::new_interactive_with_managed_components_for_test(
            host.clone(),
            SpyManagedDownloader::new(HashMap::new()),
            paths.clone(),
        );

        ChromeInstaller::new(
            paths,
            SpyManagedDownloader::with_zip_bytes(fake_driver_zip()),
        )
        .ensure_driver("124")
        .await
        .unwrap();

        tool.execute(
            json!({
                "action": "navigate",
                "url": "https://example.com"
            }),
            make_ctx(),
        )
        .await
        .expect("interactive navigate should succeed");

        let open_call = host
            .open_calls
            .lock()
            .unwrap()
            .last()
            .expect("host should receive an open call")
            .clone();
        assert_eq!(open_call.session_mode, SessionMode::Interactive);
    }

    #[tokio::test]
    async fn managed_constructor_reuses_shared_session_on_second_navigate() {
        let home = tempdir().unwrap();
        let paths = ChromePaths::from_home(home.path());
        let host = Arc::new(FakeManagedChromeHost::new(
            home.path().join("Google Chrome"),
            "124",
            "Managed Example",
        ));
        let tool = ChromeTool::new_with_managed_components_for_test(
            host.clone(),
            SpyManagedDownloader::new(HashMap::new()),
            paths.clone(),
        );

        ChromeInstaller::new(
            paths,
            SpyManagedDownloader::with_zip_bytes(fake_driver_zip()),
        )
        .ensure_driver("124")
        .await
        .unwrap();

        tool.execute(
            json!({
                "action": "navigate",
                "url": "https://example.com/one"
            }),
            make_ctx(),
        )
        .await
        .expect("first navigate should succeed");
        tool.execute(
            json!({
                "action": "navigate",
                "url": "https://example.com/two"
            }),
            make_ctx(),
        )
        .await
        .expect("second navigate should succeed");

        assert_eq!(host.open_calls.lock().unwrap().len(), 1);

        let result = tool
            .execute(
                json!({
                    "action": "extract_text",
                }),
                make_ctx(),
            )
            .await
            .expect("session should still be alive");
        assert_eq!(result["action"], "extract_text");
    }

    #[test]
    fn chrome_paths_use_arguswing_root() {
        let paths = ChromePaths::from_home(Path::new("/tmp/home"));
        assert_eq!(paths.root, PathBuf::from("/tmp/home/.arguswing/chrome"));
    }

    #[test]
    fn chrome_paths_include_shared_session_state_file() {
        let paths = ChromePaths::from_home(Path::new("/tmp/home"));
        assert_eq!(
            paths.shared_session_state,
            PathBuf::from("/tmp/home/.arguswing/chrome/shared-session.json")
        );
    }

    #[tokio::test]
    async fn managed_constructor_recovers_shared_session_from_state_file() {
        let home = tempdir().unwrap();
        let paths = ChromePaths::from_home(home.path());
        paths.ensure_directories().unwrap();
        std::fs::write(
            &paths.shared_session_state,
            r#"{"session_id":"persisted-session"}"#,
        )
        .unwrap();
        let host = Arc::new(
            FakeManagedChromeHost::new(home.path().join("Google Chrome"), "124", "Managed Example")
                .with_attached_session("persisted-session", "Recovered text"),
        );
        let tool = ChromeTool::new_with_managed_components_for_test(
            host.clone(),
            SpyManagedDownloader::new(HashMap::new()),
            paths,
        );

        let result = tool
            .execute(json!({ "action": "extract_text" }), make_ctx())
            .await
            .expect("extract_text should recover persisted shared session");

        assert_eq!(result["content"], "Recovered text");
        assert_eq!(
            host.attach_calls.lock().unwrap().as_slice(),
            &["persisted-session"]
        );
    }

    #[tokio::test]
    async fn managed_constructor_requires_navigate_when_persisted_session_is_stale() {
        let home = tempdir().unwrap();
        let paths = ChromePaths::from_home(home.path());
        paths.ensure_directories().unwrap();
        std::fs::write(
            &paths.shared_session_state,
            r#"{"session_id":"stale-session"}"#,
        )
        .unwrap();
        let host = Arc::new(FakeManagedChromeHost::new(
            home.path().join("Google Chrome"),
            "124",
            "Managed Example",
        ));
        let tool = ChromeTool::new_with_managed_components_for_test(
            host.clone(),
            SpyManagedDownloader::new(HashMap::new()),
            paths,
        );

        let err = tool
            .execute(json!({ "action": "extract_text" }), make_ctx())
            .await
            .expect_err("stale persisted session should require navigate");

        assert!(matches!(
            err,
            ToolError::ExecutionFailed { reason, .. }
            if reason.contains("navigate(url)")
        ));
        assert_eq!(
            host.attach_calls.lock().unwrap().as_slice(),
            &["stale-session"]
        );
    }

    #[tokio::test]
    async fn managed_constructor_recreates_stale_shared_session_on_navigate() {
        let home = tempdir().unwrap();
        let paths = ChromePaths::from_home(home.path());
        paths.ensure_directories().unwrap();
        std::fs::write(
            &paths.shared_session_state,
            r#"{"session_id":"stale-session"}"#,
        )
        .unwrap();
        let host = Arc::new(FakeManagedChromeHost::new(
            home.path().join("Google Chrome"),
            "124",
            "Managed Example",
        ));
        let tool = ChromeTool::new_with_managed_components_for_test(
            host.clone(),
            SpyManagedDownloader::with_zip_bytes(fake_driver_zip()),
            paths,
        );

        ChromeInstaller::new(
            ChromePaths::from_home(home.path()),
            SpyManagedDownloader::with_zip_bytes(fake_driver_zip()),
        )
        .ensure_driver("124")
        .await
        .unwrap();

        let result = tool
            .execute(
                json!({
                    "action": "navigate",
                    "url": "https://example.com"
                }),
                make_ctx(),
            )
            .await
            .expect("navigate should recreate stale persisted session");

        assert_eq!(result["page_title"], "Managed Example");
        assert_eq!(
            host.attach_calls.lock().unwrap().as_slice(),
            &["stale-session"]
        );
        assert_eq!(host.open_calls.lock().unwrap().len(), 1);
    }

    #[test]
    fn patcher_rewrites_cdc_tokens() {
        let input = b"aaaaacdc_123456789012345678zz".to_vec();
        let output = patch_cdc_tokens(input, b'X').unwrap();

        let expected = b"aaaaaXXXXXXXXXXXXXXXXXXXXXXzz".to_vec();
        assert_eq!(output, expected);
        assert!(output.starts_with(b"aaaaa"));
        assert!(output.ends_with(b"zz"));
    }

    #[derive(Debug, Clone)]
    struct ManagedOpenCall {
        browser_binary: PathBuf,
        driver_binary: PathBuf,
        session_mode: SessionMode,
    }

    #[derive(Debug, Clone)]
    struct AttachedManagedSession {
        page_title: String,
        text: String,
    }

    struct FakeManagedChromeHost {
        browser_binary: PathBuf,
        browser_version: String,
        page_title: String,
        open_calls: StdMutex<Vec<ManagedOpenCall>>,
        attach_calls: StdMutex<Vec<String>>,
        attached_sessions: StdMutex<HashMap<String, AttachedManagedSession>>,
    }

    impl FakeManagedChromeHost {
        fn new(
            browser_binary: PathBuf,
            browser_version: impl Into<String>,
            page_title: impl Into<String>,
        ) -> Self {
            Self {
                browser_binary,
                browser_version: browser_version.into(),
                page_title: page_title.into(),
                open_calls: StdMutex::new(Vec::new()),
                attach_calls: StdMutex::new(Vec::new()),
                attached_sessions: StdMutex::new(HashMap::new()),
            }
        }

        fn with_attached_session(
            self,
            session_id: impl Into<String>,
            text: impl Into<String>,
        ) -> Self {
            self.attached_sessions.lock().unwrap().insert(
                session_id.into(),
                AttachedManagedSession {
                    page_title: self.page_title.clone(),
                    text: text.into(),
                },
            );
            self
        }
    }

    #[async_trait::async_trait]
    impl ChromeHost for FakeManagedChromeHost {
        async fn discover_chrome(&self) -> Result<DetectedChrome, ChromeToolError> {
            Ok(DetectedChrome {
                browser_binary: self.browser_binary.clone(),
                browser_version: self.browser_version.clone(),
            })
        }

        async fn open_session(
            &self,
            url: &str,
            browser_binary: &Path,
            _browser_version: &str,
            driver_binary: &Path,
            session_mode: SessionMode,
        ) -> Result<BackendOpenResult, ChromeToolError> {
            self.open_calls.lock().unwrap().push(ManagedOpenCall {
                browser_binary: browser_binary.to_path_buf(),
                driver_binary: driver_binary.to_path_buf(),
                session_mode,
            });

            let session: Arc<dyn BrowserSession> = Arc::new(FakeBrowserSession {
                text: "Managed text".to_string(),
                url: url.to_string(),
                cookies: vec![],
                tabs: StdMutex::new(vec![FakeTab {
                    handle: "tab-1".to_string(),
                    url: url.to_string(),
                    title: self.page_title.clone(),
                }]),
            });

            Ok(BackendOpenResult {
                backend_session_id: Some(format!(
                    "managed-session-{}",
                    self.open_calls.lock().unwrap().len()
                )),
                metadata: PageMetadata {
                    final_url: url.to_string(),
                    page_title: self.page_title.clone(),
                },
                session,
            })
        }

        async fn attach_session(
            &self,
            session_id: &str,
            _session_mode: SessionMode,
        ) -> Result<BackendOpenResult, ChromeToolError> {
            self.attach_calls
                .lock()
                .unwrap()
                .push(session_id.to_string());
            let attached = self
                .attached_sessions
                .lock()
                .unwrap()
                .get(session_id)
                .cloned()
                .ok_or(ChromeToolError::SharedSessionUnavailable)?;

            let session: Arc<dyn BrowserSession> = Arc::new(FakeBrowserSession {
                text: attached.text,
                url: "https://example.com/recovered".to_string(),
                cookies: vec![],
                tabs: StdMutex::new(vec![FakeTab {
                    handle: "tab-1".to_string(),
                    url: "https://example.com/recovered".to_string(),
                    title: attached.page_title.clone(),
                }]),
            });

            Ok(BackendOpenResult {
                backend_session_id: Some(session_id.to_string()),
                metadata: PageMetadata {
                    final_url: "https://example.com/recovered".to_string(),
                    page_title: attached.page_title,
                },
                session,
            })
        }
    }

    struct SpyManagedDownloader {
        responses: HashMap<String, Vec<u8>>,
        requests: StdMutex<Vec<String>>,
    }

    impl SpyManagedDownloader {
        fn new(responses: HashMap<String, Vec<u8>>) -> Arc<Self> {
            Arc::new(Self {
                responses,
                requests: StdMutex::new(Vec::new()),
            })
        }

        fn with_zip_bytes(zip_bytes: Vec<u8>) -> Arc<Self> {
            let mut responses = HashMap::new();
            responses.insert(
                "latest-versions-per-milestone.json".to_string(),
                br#"{"milestones":{"124":{"version":"124.0.6367.91"}}}"#.to_vec(),
            );
            responses.insert("chromedriver-".to_string(), zip_bytes);
            Self::new(responses)
        }
    }

    #[async_trait::async_trait]
    impl DriverDownloader for SpyManagedDownloader {
        async fn fetch(&self, url: &str) -> Result<Vec<u8>, ChromeToolError> {
            self.requests.lock().unwrap().push(url.to_string());
            self.responses
                .iter()
                .find_map(|(needle, value)| url.contains(needle).then(|| value.clone()))
                .ok_or_else(|| ChromeToolError::DriverDownloadFailed {
                    url: url.to_string(),
                    reason: "missing fake managed response".to_string(),
                })
        }
    }

    fn fake_driver_zip() -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();
        writer
            .start_file("chromedriver-linux64/chromedriver", options)
            .unwrap();
        writer
            .write_all(b"binary-with-cdc_123456789012345678-marker")
            .unwrap();
        writer
            .start_file("chromedriver-win64/chromedriver.exe", options)
            .unwrap();
        writer.write_all(b"windows-binary").unwrap();
        writer.finish().unwrap().into_inner()
    }

    #[derive(Default)]
    struct FakeChromeManager;

    #[async_trait::async_trait]
    impl BrowserBackend for FakeChromeManager {
        async fn open(&self, _url: &str) -> Result<BackendOpenResult, ChromeToolError> {
            Err(ChromeToolError::InvalidArguments {
                reason: "fake chrome backend should not be used".to_string(),
            })
        }
    }

    #[derive(Debug, Default)]
    struct FakeChromeBackend {
        pages: HashMap<String, FakePage>,
    }

    impl FakeChromeBackend {
        fn with_page(
            mut self,
            requested_url: impl Into<String>,
            final_url: impl Into<String>,
            page_title: impl Into<String>,
            _links: Vec<()>,
            text: impl Into<String>,
        ) -> Self {
            self.pages.insert(
                requested_url.into(),
                FakePage {
                    final_url: final_url.into(),
                    page_title: page_title.into(),
                    text: text.into(),
                    cookies: Vec::new(),
                },
            );
            self
        }

        fn with_page_and_cookies(
            mut self,
            requested_url: impl Into<String>,
            final_url: impl Into<String>,
            page_title: impl Into<String>,
            _links: Vec<()>,
            text: impl Into<String>,
            cookies: Vec<Cookie>,
        ) -> Self {
            self.pages.insert(
                requested_url.into(),
                FakePage {
                    final_url: final_url.into(),
                    page_title: page_title.into(),
                    text: text.into(),
                    cookies,
                },
            );
            self
        }
    }

    #[derive(Debug, Clone)]
    struct FakePage {
        final_url: String,
        page_title: String,
        text: String,
        cookies: Vec<Cookie>,
    }

    #[derive(Debug)]
    struct FakeBrowserSession {
        text: String,
        url: String,
        cookies: Vec<Cookie>,
        tabs: StdMutex<Vec<FakeTab>>,
    }

    #[derive(Debug, Clone)]
    struct FakeTab {
        handle: String,
        url: String,
        title: String,
    }

    #[async_trait::async_trait]
    impl BrowserSession for FakeBrowserSession {
        async fn extract_text(&self, selector: Option<&str>) -> Result<String, ChromeToolError> {
            Ok(match selector {
                Some(selector) => format!("{} [{selector}]", self.text),
                None => self.text.clone(),
            })
        }

        async fn shutdown(&self) -> Result<(), ChromeToolError> {
            Ok(())
        }

        async fn click(&self, _selector: &str) -> Result<(), ChromeToolError> {
            Ok(())
        }

        async fn type_text(&self, _selector: &str, _text: &str) -> Result<(), ChromeToolError> {
            Ok(())
        }

        async fn current_url(&self) -> Result<String, ChromeToolError> {
            Ok(self.url.clone())
        }

        async fn get_cookies(&self) -> Result<Vec<Cookie>, ChromeToolError> {
            Ok(self.cookies.clone())
        }

        async fn navigate(&self, url: &str) -> Result<PageMetadata, ChromeToolError> {
            Ok(PageMetadata {
                final_url: url.to_string(),
                page_title: format!("Navigated to {url}"),
            })
        }

        async fn create_new_tab(
            &self,
            url: &str,
        ) -> Result<(String, PageMetadata), ChromeToolError> {
            let handle = format!("tab-{}", self.tabs.lock().unwrap().len() + 2);
            let metadata = PageMetadata {
                final_url: url.to_string(),
                page_title: format!("Tab {url}"),
            };
            self.tabs.lock().unwrap().push(FakeTab {
                handle: handle.clone(),
                url: metadata.final_url.clone(),
                title: metadata.page_title.clone(),
            });
            Ok((handle, metadata))
        }

        async fn switch_to_window(
            &self,
            window_handle: &str,
        ) -> Result<PageMetadata, ChromeToolError> {
            let tabs = self.tabs.lock().unwrap();
            let tab = tabs
                .iter()
                .find(|t| t.handle == window_handle)
                .ok_or_else(|| ChromeToolError::TabNotFound {
                    tab_id: window_handle.to_string(),
                })?;
            Ok(PageMetadata {
                final_url: tab.url.clone(),
                page_title: tab.title.clone(),
            })
        }

        async fn close_current_window(&self) -> Result<(), ChromeToolError> {
            Ok(())
        }

        async fn list_windows(&self) -> Result<Vec<(String, String, String)>, ChromeToolError> {
            let tabs = self.tabs.lock().unwrap();
            Ok(tabs
                .iter()
                .map(|t| (t.handle.clone(), t.url.clone(), t.title.clone()))
                .collect())
        }

        async fn current_window_handle(&self) -> Result<String, ChromeToolError> {
            let tabs = self.tabs.lock().unwrap();
            tabs.first().map(|t| t.handle.clone()).ok_or_else(|| {
                ChromeToolError::TabOperationFailed {
                    reason: "no tabs".to_string(),
                }
            })
        }
    }

    #[async_trait::async_trait]
    impl BrowserBackend for FakeChromeBackend {
        async fn open(&self, url: &str) -> Result<BackendOpenResult, ChromeToolError> {
            let page = self
                .pages
                .get(url)
                .ok_or_else(|| ChromeToolError::InvalidArguments {
                    reason: format!("no fake page for url '{url}'"),
                })?;

            let session: Arc<dyn BrowserSession> = Arc::new(FakeBrowserSession {
                text: page.text.clone(),
                url: page.final_url.clone(),
                cookies: page.cookies.clone(),
                tabs: StdMutex::new(vec![FakeTab {
                    handle: "tab-1".to_string(),
                    url: page.final_url.clone(),
                    title: page.page_title.clone(),
                }]),
            });

            Ok(BackendOpenResult {
                backend_session_id: None,
                metadata: PageMetadata {
                    final_url: page.final_url.clone(),
                    page_title: page.page_title.clone(),
                },
                session,
            })
        }
    }
}
