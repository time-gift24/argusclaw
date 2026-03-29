use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use tokio::sync::RwLock;

use super::error::ChromeToolError;
use super::models::{LinkSummary, OpenArgs, OpenedSession, PageMetadata};
use super::session::{BrowserSession, ChromeSession};

pub struct BackendOpenResult {
    pub metadata: PageMetadata,
    pub session: Arc<dyn BrowserSession>,
}

#[async_trait]
pub trait BrowserBackend: Send + Sync {
    async fn open(&self, url: &str) -> Result<BackendOpenResult, ChromeToolError>;
}

pub struct ChromeManager {
    backend: Arc<dyn BrowserBackend>,
    sessions: RwLock<HashMap<String, ChromeSession>>,
    next_session_id: AtomicU64,
}

impl ChromeManager {
    #[must_use]
    pub fn new(backend: Arc<dyn BrowserBackend>) -> Self {
        Self {
            backend,
            sessions: RwLock::new(HashMap::new()),
            next_session_id: AtomicU64::new(0),
        }
    }

    #[must_use]
    pub fn new_for_test(backend: Arc<dyn BrowserBackend>) -> Self {
        Self::new(backend)
    }

    pub async fn open(&self, args: OpenArgs) -> Result<OpenedSession, ChromeToolError> {
        let opened = self.backend.open(&args.url).await?;
        let session_id = self.next_session_id();

        let session = ChromeSession::new(
            session_id.clone(),
            opened.metadata.final_url.clone(),
            opened.metadata.page_title.clone(),
            opened.session,
        );
        self.sessions.write().await.insert(session_id.clone(), session);

        Ok(OpenedSession {
            session_id,
            final_url: opened.metadata.final_url,
            page_title: opened.metadata.page_title,
        })
    }

    pub async fn session(&self, session_id: &str) -> Result<ChromeSession, ChromeToolError> {
        self.sessions
            .read()
            .await
            .get(session_id)
            .cloned()
            .ok_or_else(|| Self::session_not_found(session_id))
    }

    pub async fn list_links(&self, session_id: &str) -> Result<Vec<LinkSummary>, ChromeToolError> {
        self.session_interaction(session_id).await?.list_links().await
    }

    pub async fn extract_text(
        &self,
        session_id: &str,
        selector: Option<&str>,
    ) -> Result<String, ChromeToolError> {
        self.session_interaction(session_id)
            .await?
            .extract_text(selector)
            .await
    }

    pub async fn get_dom_summary(&self, session_id: &str) -> Result<String, ChromeToolError> {
        self.extract_text(session_id, None).await
    }

    pub async fn screenshot(
        &self,
        session_id: &str,
        screenshot_path: PathBuf,
    ) -> Result<PathBuf, ChromeToolError> {
        let mut sessions = self.sessions.write().await;
        let session = sessions
            .get_mut(session_id)
            .ok_or_else(|| Self::session_not_found(session_id))?;
        session.set_last_screenshot_path(Some(screenshot_path.clone()));
        Ok(screenshot_path)
    }

    fn next_session_id(&self) -> String {
        let next = self.next_session_id.fetch_add(1, Ordering::Relaxed) + 1;
        format!("session-{next}")
    }

    async fn session_interaction(
        &self,
        session_id: &str,
    ) -> Result<Arc<dyn BrowserSession>, ChromeToolError> {
        self.sessions
            .read()
            .await
            .get(session_id)
            .map(ChromeSession::interaction)
            .ok_or_else(|| Self::session_not_found(session_id))
    }

    fn session_not_found(session_id: &str) -> ChromeToolError {
        ChromeToolError::SessionNotFound {
            session_id: session_id.to_string(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::sync::Arc;

    use crate::chrome::error::ChromeToolError;
    use crate::chrome::models::{LinkSummary, OpenArgs, PageMetadata};
    use crate::chrome::session::BrowserSession;

    use super::{BackendOpenResult, BrowserBackend, ChromeManager};

    #[derive(Debug, Clone)]
    struct FakePage {
        final_url: String,
        page_title: String,
        links: Vec<LinkSummary>,
        text: String,
    }

    #[derive(Debug, Default)]
    struct FakeBrowserBackend {
        pages: HashMap<String, FakePage>,
    }

    impl FakeBrowserBackend {
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
    impl BrowserBackend for FakeBrowserBackend {
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

    fn sample_backend() -> Arc<FakeBrowserBackend> {
        Arc::new(
            FakeBrowserBackend::default()
                .with_page(
                    "https://example.com",
                    "https://example.com",
                    "Example",
                    vec![LinkSummary {
                        href: "https://example.com/about".to_string(),
                        text: "About".to_string(),
                    }],
                    "Example text",
                )
                .with_page(
                    "https://example.org",
                    "https://example.org/home",
                    "Example Org",
                    vec![LinkSummary {
                        href: "https://example.org/docs".to_string(),
                        text: "Docs".to_string(),
                    }],
                    "Org text",
                ),
        )
    }

    #[tokio::test]
    async fn manager_creates_session_and_returns_metadata() {
        let manager = ChromeManager::new_for_test(sample_backend());

        let opened = manager
            .open(OpenArgs {
                url: "https://example.com".into(),
            })
            .await
            .unwrap();

        assert_eq!(opened.final_url, "https://example.com");
        assert_eq!(opened.page_title, "Example");
        assert!(!opened.session_id.is_empty());
    }

    #[tokio::test]
    async fn manager_stores_opened_session_and_returns_it() {
        let manager = ChromeManager::new_for_test(sample_backend());

        let opened = manager
            .open(OpenArgs {
                url: "https://example.com".into(),
            })
            .await
            .unwrap();

        let session = manager.session(&opened.session_id).await.unwrap();

        assert_eq!(session.session_id, opened.session_id);
        assert_eq!(session.current_url, "https://example.com");
        assert_eq!(session.page_title, "Example");
        assert_eq!(session.last_screenshot_path, None);
    }

    #[tokio::test]
    async fn manager_rejects_unknown_session_with_variant() {
        let manager = ChromeManager::new_for_test(sample_backend());
        let err = manager.session("missing").await.unwrap_err();
        assert!(matches!(
            err,
            ChromeToolError::SessionNotFound { session_id } if session_id == "missing"
        ));
    }

    #[tokio::test]
    async fn manager_uses_session_handle_for_read_operations() {
        let manager = ChromeManager::new_for_test(sample_backend());
        let first = manager
            .open(OpenArgs {
                url: "https://example.com".into(),
            })
            .await
            .unwrap();
        let second = manager
            .open(OpenArgs {
                url: "https://example.org".into(),
            })
            .await
            .unwrap();

        let first_links = manager.list_links(&first.session_id).await.unwrap();
        let second_links = manager.list_links(&second.session_id).await.unwrap();
        assert_eq!(first_links[0].href, "https://example.com/about");
        assert_eq!(second_links[0].href, "https://example.org/docs");

        let first_text = manager
            .extract_text(&first.session_id, Some("#hero"))
            .await
            .unwrap();
        let second_summary = manager.get_dom_summary(&second.session_id).await.unwrap();
        assert_eq!(first_text, "Example text [#hero]");
        assert_eq!(second_summary, "Org text");
    }

    #[tokio::test]
    async fn manager_screenshot_updates_session_state() {
        let manager = ChromeManager::new_for_test(sample_backend());
        let opened = manager
            .open(OpenArgs {
                url: "https://example.com".into(),
            })
            .await
            .unwrap();

        let screenshot_path = PathBuf::from("/tmp/example.png");
        let returned = manager
            .screenshot(&opened.session_id, screenshot_path.clone())
            .await
            .unwrap();
        assert_eq!(returned, screenshot_path);

        let session = manager.session(&opened.session_id).await.unwrap();
        assert_eq!(session.last_screenshot_path, Some(screenshot_path));
    }

    #[tokio::test]
    async fn manager_api_rejects_missing_session_for_all_session_ops() {
        let manager = ChromeManager::new_for_test(sample_backend());

        let err = manager.list_links("missing").await.unwrap_err();
        assert!(matches!(
            err,
            ChromeToolError::SessionNotFound { session_id } if session_id == "missing"
        ));

        let err = manager.extract_text("missing", None).await.unwrap_err();
        assert!(matches!(
            err,
            ChromeToolError::SessionNotFound { session_id } if session_id == "missing"
        ));

        let err = manager.get_dom_summary("missing").await.unwrap_err();
        assert!(matches!(
            err,
            ChromeToolError::SessionNotFound { session_id } if session_id == "missing"
        ));

        let err = manager
            .screenshot("missing", PathBuf::from("/tmp/missing.png"))
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ChromeToolError::SessionNotFound { session_id } if session_id == "missing"
        ));
    }
}
