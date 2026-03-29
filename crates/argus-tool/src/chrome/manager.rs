use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;
use tokio::sync::RwLock;

use super::error::ChromeToolError;
use super::models::{LinkSummary, OpenArgs, OpenedSession, PageMetadata};
use super::session::ChromeSession;

#[async_trait]
pub trait BrowserBackend: Send + Sync {
    async fn open(&self, url: &str) -> Result<PageMetadata, ChromeToolError>;
    async fn extract_text(&self, selector: Option<&str>) -> Result<String, ChromeToolError>;
    async fn list_links(&self) -> Result<Vec<LinkSummary>, ChromeToolError>;
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
        let metadata = self.backend.open(&args.url).await?;
        let session_id = self.next_session_id();

        let session = ChromeSession::new(
            session_id.clone(),
            metadata.final_url.clone(),
            metadata.page_title.clone(),
        );
        self.sessions.write().await.insert(session_id.clone(), session);

        Ok(OpenedSession {
            session_id,
            final_url: metadata.final_url,
            page_title: metadata.page_title,
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
        self.ensure_session_exists(session_id).await?;
        self.backend.list_links().await
    }

    pub async fn extract_text(
        &self,
        session_id: &str,
        selector: Option<&str>,
    ) -> Result<String, ChromeToolError> {
        self.ensure_session_exists(session_id).await?;
        self.backend.extract_text(selector).await
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
        session.last_screenshot_path = Some(screenshot_path.clone());
        Ok(screenshot_path)
    }

    fn next_session_id(&self) -> String {
        let next = self.next_session_id.fetch_add(1, Ordering::Relaxed) + 1;
        format!("session-{next}")
    }

    async fn ensure_session_exists(&self, session_id: &str) -> Result<(), ChromeToolError> {
        if self.sessions.read().await.contains_key(session_id) {
            Ok(())
        } else {
            Err(Self::session_not_found(session_id))
        }
    }

    fn session_not_found(session_id: &str) -> ChromeToolError {
        ChromeToolError::InvalidArguments {
            reason: format!("SessionNotFound: session '{session_id}' does not exist"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use crate::chrome::error::ChromeToolError;
    use crate::chrome::models::{LinkSummary, OpenArgs, PageMetadata};

    use super::{BrowserBackend, ChromeManager};

    #[derive(Debug, Default)]
    struct FakeBrowserBackend {
        final_url: String,
        page_title: String,
    }

    impl FakeBrowserBackend {
        fn new(final_url: impl Into<String>, page_title: impl Into<String>) -> Self {
            Self {
                final_url: final_url.into(),
                page_title: page_title.into(),
            }
        }
    }

    #[async_trait::async_trait]
    impl BrowserBackend for FakeBrowserBackend {
        async fn open(&self, _url: &str) -> Result<PageMetadata, ChromeToolError> {
            Ok(PageMetadata {
                final_url: self.final_url.clone(),
                page_title: self.page_title.clone(),
            })
        }

        async fn extract_text(&self, _selector: Option<&str>) -> Result<String, ChromeToolError> {
            Ok("fake text".to_string())
        }

        async fn list_links(&self) -> Result<Vec<LinkSummary>, ChromeToolError> {
            Ok(Vec::new())
        }
    }

    #[tokio::test]
    async fn manager_creates_session_and_returns_metadata() {
        let backend = Arc::new(FakeBrowserBackend::new("https://example.com", "Example"));
        let manager = ChromeManager::new_for_test(backend);

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
    async fn manager_rejects_unknown_session() {
        let manager = ChromeManager::new_for_test(Arc::new(FakeBrowserBackend::default()));
        let err = manager.session("missing").await.unwrap_err();
        assert!(err.to_string().contains("SessionNotFound"));
    }
}
