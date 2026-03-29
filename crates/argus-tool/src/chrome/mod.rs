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
    use std::sync::Arc;

    use std::path::{Path, PathBuf};

    use serde_json::json;

    use super::error::ChromeToolError;
    use super::installer::ChromePaths;
    use super::manager::{BackendOpenResult, BrowserBackend};
    use super::models::{ChromeAction, ChromeToolArgs, LinkSummary, PageMetadata};
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
    fn click_rejects_url_argument() {
        let err = ChromeToolArgs::validate(json!({
            "action": "click",
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
        let tool = ChromeTool::new_for_test(Arc::new(FakeChromeManager::default()));
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
                .is_some()
        );
    }

    #[tokio::test]
    async fn chrome_tool_rejects_denied_action_before_backend() {
        let tool = ChromeTool::new_for_test(Arc::new(FakeChromeManager::default()));
        let err = tool
            .execute(json!({ "action": "click" }), make_ctx())
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
                    "session_id": session_id,
                    "screenshot_path": "/tmp/example.png"
                }),
                make_ctx(),
            )
            .await
            .expect("screenshot should succeed");
        assert_eq!(screenshot["screenshot_path"], "/tmp/example.png");
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
    }

    #[derive(Debug)]
    struct FakeBrowserSession {
        links: Vec<LinkSummary>,
        text: String,
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
