mod error;
mod install_tool;
mod installer;
mod manager;
mod models;
mod patcher;
mod policy;
mod session;
mod tool;

pub use install_tool::ChromeInstallTool;
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
    use zip::write::SimpleFileOptions;

    use super::error::ChromeToolError;
    use super::install_tool::ChromeInstallTool;
    use super::installer::{ChromeInstaller, ChromePaths, DriverDownloader};
    use super::manager::{
        BackendOpenResult, BrowserBackend, ChromeHost, DetectedChrome, SessionMode,
    };
    use super::models::{ChromeAction, ChromeToolArgs, CookieSummary, LinkSummary, PageMetadata};
    use super::patcher::patch_cdc_tokens;
    use super::policy::ExplorePolicy;
    use super::session::BrowserSession;
    use super::tool::ChromeTool;
    use argus_protocol::NamedTool;
    use argus_protocol::ToolExecutionContext;
    use argus_protocol::ids::ThreadId;
    use argus_protocol::tool::ToolError;
    use tokio::sync::broadcast;
    use tokio::sync::mpsc;

    fn make_ctx() -> Arc<ToolExecutionContext> {
        let (pipe_tx, _) = broadcast::channel(16);
        let (control_tx, _control_rx) = mpsc::unbounded_channel();
        Arc::new(ToolExecutionContext {
            thread_id: ThreadId::new(),
            pipe_tx,
            control_tx,
        })
    }

    #[test]
    fn open_requires_url() {
        let err = ChromeToolArgs::validate(json!({ "action": "open" })).unwrap_err();
        assert!(matches!(
            err,
            ChromeToolError::MissingRequiredField { action, field }
            if action == "open" && field == "url"
        ));
    }

    #[test]
    fn open_rejects_malformed_url() {
        let err =
            ChromeToolArgs::validate(json!({ "action": "open", "url": "not-a-url" })).unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));
    }

    #[test]
    fn open_rejects_non_http_scheme() {
        let err = ChromeToolArgs::validate(json!({ "action": "open", "url": "file:///tmp/a.txt" }))
            .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));

        let err = ChromeToolArgs::validate(json!({ "action": "open", "url": "chrome://settings" }))
            .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));
    }

    #[test]
    fn open_rejects_local_or_private_targets() {
        let err =
            ChromeToolArgs::validate(json!({ "action": "open", "url": "https://localhost/path" }))
                .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));

        let err =
            ChromeToolArgs::validate(json!({ "action": "open", "url": "https://127.0.0.1/path" }))
                .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));

        let err = ChromeToolArgs::validate(json!({ "action": "open", "url": "http://10.0.0.1" }))
            .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));

        let err = ChromeToolArgs::validate(json!({ "action": "open", "url": "http://0.0.0.0" }))
            .unwrap_err();
        assert!(matches!(err, ChromeToolError::InvalidArguments { .. }));
    }

    #[test]
    fn open_stores_trimmed_validated_url() {
        let args = ChromeToolArgs::validate(
            json!({ "action": "open", "url": "  https://example.com/path?q=1  " }),
        )
        .unwrap();

        assert_eq!(args.url.as_deref(), Some("https://example.com/path?q=1"));
    }

    #[test]
    fn click_requires_session_id_and_selector() {
        // Missing session_id
        let err = ChromeToolArgs::validate(json!({
            "action": "click",
            "selector": "#btn",
        }))
        .unwrap_err();
        assert!(matches!(err, ChromeToolError::MissingRequiredField { .. }));

        // Missing selector
        let err = ChromeToolArgs::validate(json!({
            "action": "click",
            "session_id": "session-1",
        }))
        .unwrap_err();
        assert!(matches!(err, ChromeToolError::MissingRequiredField { .. }));

        // url not allowed
        let err = ChromeToolArgs::validate(json!({
            "action": "click",
            "session_id": "session-1",
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
            "session_id": "session-1",
            "selector": "#hero"
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
    fn list_links_is_allowed() {
        ExplorePolicy::readonly()
            .validate_action(ChromeAction::ListLinks)
            .unwrap();
    }

    #[test]
    fn chrome_tool_definition_lists_only_readonly_actions() {
        let tool = ChromeTool::new_for_test(Arc::new(FakeChromeManager));
        let def = tool.definition();
        assert_eq!(def.name, "chrome");
        assert!(def.description.contains("read-only"));

        let action_enum = def.parameters["properties"]["action"]["enum"]
            .as_array()
            .expect("action enum should be present");
        let action_values: Vec<&str> = action_enum
            .iter()
            .map(|value| value.as_str().expect("enum value should be a string"))
            .collect();
        assert!(action_values.contains(&"open"));
        assert!(action_values.contains(&"wait"));
        assert!(action_values.contains(&"extract_text"));
        assert!(action_values.contains(&"list_links"));
        assert!(action_values.contains(&"get_dom_summary"));
        assert!(action_values.contains(&"screenshot"));
        assert!(!action_values.contains(&"click"));

        assert!(def.parameters["properties"].get("session_id").is_some());
        assert!(def.parameters["properties"].get("selector").is_some());
        assert!(
            def.parameters["properties"]
                .get("screenshot_path")
                .is_none()
        );
        assert!(def.parameters["properties"].get("text").is_none());
    }

    #[test]
    fn chrome_tool_definition_lists_interactive_actions() {
        let tool = ChromeTool::new_interactive_with_backend(Arc::new(FakeChromeManager));
        let def = tool.definition();
        assert_eq!(def.name, "chrome");
        assert!(def.description.contains("interactive"));

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
        assert!(def.parameters["properties"].get("text").is_some());
    }

    #[tokio::test]
    async fn chrome_tool_rejects_denied_action_before_backend() {
        let tool = ChromeTool::new_for_test(Arc::new(FakeChromeManager));
        // Click is blocked by readonly policy even with valid args
        let err = tool
            .execute(
                json!({
                    "action": "click",
                    "session_id": "session-1",
                    "selector": "#btn",
                }),
                make_ctx(),
            )
            .await
            .unwrap_err();
        assert!(matches!(err, ToolError::NotAuthorized(_)));
    }

    #[tokio::test]
    async fn chrome_tool_dispatches_read_actions_through_manager() {
        let tool = ChromeTool::new_for_test(Arc::new(FakeChromeBackend::default().with_page(
            "https://example.com",
            "https://example.com/landing",
            "Example Title",
            vec![LinkSummary {
                href: "https://example.com/docs".to_string(),
                text: "Docs".to_string(),
            }],
            "Visible page text",
        )));

        let open = tool
            .execute(
                json!({
                    "action": "open",
                    "url": "https://example.com"
                }),
                make_ctx(),
            )
            .await
            .expect("open should succeed");

        let session_id = open["session_id"]
            .as_str()
            .expect("open should return a session id")
            .to_string();

        let wait = tool
            .execute(
                json!({
                    "action": "wait",
                    "session_id": session_id
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
                    "session_id": session_id,
                    "selector": "main"
                }),
                make_ctx(),
            )
            .await
            .expect("extract_text should succeed");
        assert_eq!(extract["content"], "Visible page text [main]");

        let links = tool
            .execute(
                json!({
                    "action": "list_links",
                    "session_id": session_id
                }),
                make_ctx(),
            )
            .await
            .expect("list_links should succeed");
        assert_eq!(links["links"].as_array().map(|links| links.len()), Some(1));
        assert_eq!(links["links"][0]["text"], "Docs");

        let summary = tool
            .execute(
                json!({
                    "action": "get_dom_summary",
                    "session_id": session_id
                }),
                make_ctx(),
            )
            .await
            .expect("get_dom_summary should succeed");
        assert_eq!(summary["summary"], "Visible page text");

        let screenshot = tool
            .execute(
                json!({
                    "action": "screenshot",
                    "session_id": session_id
                }),
                make_ctx(),
            )
            .await
            .expect("screenshot should succeed");
        let screenshot_path = PathBuf::from(
            screenshot["screenshot_path"]
                .as_str()
                .expect("screenshot path should be returned"),
        );
        assert!(screenshot_path.is_absolute());
        assert!(screenshot_path.is_file());
        assert_eq!(
            screenshot_path.extension().and_then(|value| value.to_str()),
            Some("png")
        );
    }

    #[tokio::test]
    async fn chrome_install_tool_installs_and_reports_result() {
        let home = tempdir().unwrap();
        let paths = ChromePaths::from_home(home.path());
        let host = Arc::new(FakeManagedChromeHost::new(
            home.path().join("Google Chrome"),
            "124",
            "Managed Example",
        ));
        let downloader = SpyManagedDownloader::with_zip_bytes(fake_driver_zip());
        let tool = ChromeInstallTool::new_with_components_for_test(host, downloader, paths.clone());

        let result = tool.execute(json!({}), make_ctx()).await.unwrap();

        assert_eq!(result["browser_version"], "124");
        assert_eq!(result["driver_version"], "124.0.6367.91");
        assert_eq!(result["cache_hit"], false);
        let driver_path = PathBuf::from(result["driver_path"].as_str().unwrap());
        assert!(driver_path.starts_with(&paths.patched));
        assert!(driver_path.is_file());
    }

    #[tokio::test]
    async fn chrome_install_tool_reuses_cached_driver_without_network() {
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
        let tool = ChromeInstallTool::new_with_components_for_test(host, downloader.clone(), paths);

        let result = tool.execute(json!({}), make_ctx()).await.unwrap();

        assert_eq!(result["cache_hit"], true);
        assert!(downloader.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn managed_constructor_requires_explicit_install_before_open() {
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
                    "action": "open",
                    "url": "https://example.com"
                }),
                make_ctx(),
            )
            .await
            .expect_err("managed open should require explicit install");

        assert!(matches!(
            err,
            ToolError::ExecutionFailed { tool_name, reason }
                if tool_name == "chrome"
                    && reason.contains("chrome_install")
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

        let open = tool
            .execute(
                json!({
                    "action": "open",
                    "url": "https://example.com"
                }),
                make_ctx(),
            )
            .await
            .expect("managed open should succeed with cached driver");

        assert_eq!(open["page_title"], "Managed Example");

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
                "action": "open",
                "url": "https://example.com"
            }),
            make_ctx(),
        )
        .await
        .expect("interactive open should succeed");

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
    async fn managed_constructor_keeps_only_latest_session_live() {
        let home = tempdir().unwrap();
        let paths = ChromePaths::from_home(home.path());
        let host = Arc::new(FakeManagedChromeHost::new(
            home.path().join("Google Chrome"),
            "124",
            "Managed Example",
        ));
        let tool = ChromeTool::new_with_managed_components_for_test(
            host,
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

        let first_open = tool
            .execute(
                json!({
                    "action": "open",
                    "url": "https://example.com/one"
                }),
                make_ctx(),
            )
            .await
            .expect("first open should succeed");
        let second_open = tool
            .execute(
                json!({
                    "action": "open",
                    "url": "https://example.com/two"
                }),
                make_ctx(),
            )
            .await
            .expect("second open should succeed");

        assert_ne!(first_open["session_id"], second_open["session_id"]);

        let err = tool
            .execute(
                json!({
                    "action": "extract_text",
                    "session_id": first_open["session_id"].as_str().unwrap()
                }),
                make_ctx(),
            )
            .await
            .expect_err("previous production session should be evicted");

        assert!(matches!(
            err,
            ToolError::ExecutionFailed { tool_name, reason }
                if tool_name == "chrome" && reason.contains("session not found")
        ));
    }

    #[test]
    fn chrome_paths_use_arguswing_root() {
        let paths = ChromePaths::from_home(Path::new("/tmp/home"));
        assert_eq!(paths.root, PathBuf::from("/tmp/home/.arguswing/chrome"));
        assert_eq!(
            paths.screenshots,
            PathBuf::from("/tmp/home/.arguswing/chrome/screenshots")
        );
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

    struct FakeManagedChromeHost {
        browser_binary: PathBuf,
        browser_version: String,
        page_title: String,
        open_calls: StdMutex<Vec<ManagedOpenCall>>,
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
            }
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
                links: vec![LinkSummary {
                    href: format!("{url}/docs"),
                    text: "Docs".to_string(),
                }],
                text: "Managed text".to_string(),
                screenshot: b"managed-png".to_vec(),
                url: url.to_string(),
                cookies: vec![],
            });

            Ok(BackendOpenResult {
                metadata: PageMetadata {
                    final_url: url.to_string(),
                    page_title: self.page_title.clone(),
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
            links: Vec<LinkSummary>,
            text: impl Into<String>,
        ) -> Self {
            self.pages.insert(
                requested_url.into(),
                FakePage {
                    final_url: final_url.into(),
                    page_title: page_title.into(),
                    links,
                    text: text.into(),
                    screenshot: b"fake-png".to_vec(),
                },
            );
            self
        }
    }

    #[derive(Debug, Clone)]
    struct FakePage {
        final_url: String,
        page_title: String,
        links: Vec<LinkSummary>,
        text: String,
        screenshot: Vec<u8>,
    }

    #[derive(Debug)]
    struct FakeBrowserSession {
        links: Vec<LinkSummary>,
        text: String,
        screenshot: Vec<u8>,
        url: String,
        cookies: Vec<CookieSummary>,
    }

    #[async_trait::async_trait]
    impl BrowserSession for FakeBrowserSession {
        async fn extract_text(&self, selector: Option<&str>) -> Result<String, ChromeToolError> {
            Ok(match selector {
                Some(selector) => format!("{} [{selector}]", self.text),
                None => self.text.clone(),
            })
        }

        async fn list_links(&self) -> Result<Vec<LinkSummary>, ChromeToolError> {
            Ok(self.links.clone())
        }

        async fn screenshot_png(&self) -> Result<Vec<u8>, ChromeToolError> {
            Ok(self.screenshot.clone())
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

        async fn get_cookies(&self) -> Result<Vec<CookieSummary>, ChromeToolError> {
            Ok(self.cookies.clone())
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
                links: page.links.clone(),
                text: page.text.clone(),
                screenshot: page.screenshot.clone(),
                url: page.final_url.clone(),
                cookies: vec![],
            });

            Ok(BackendOpenResult {
                metadata: PageMetadata {
                    final_url: page.final_url.clone(),
                    page_title: page.page_title.clone(),
                },
                session,
            })
        }
    }
}
