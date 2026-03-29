use std::collections::{HashMap, VecDeque};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use thirtyfour::prelude::{ChromiumLikeCapabilities, DesiredCapabilities, WebDriver};
use tokio::process::Command;
use tokio::sync::RwLock;

use super::error::ChromeToolError;
use super::installer::{ChromeInstaller, ChromePaths, DriverDownloader, ReqwestDriverDownloader};
use super::models::{LinkSummary, OpenArgs, OpenedSession, PageMetadata};
use super::session::{
    BrowserSession, ChromeSession, ManagedWebDriverSession, shutdown_child_process,
};

pub struct BackendOpenResult {
    pub metadata: PageMetadata,
    pub session: Arc<dyn BrowserSession>,
}

#[async_trait]
pub trait BrowserBackend: Send + Sync {
    async fn open(&self, url: &str) -> Result<BackendOpenResult, ChromeToolError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedChrome {
    pub browser_binary: PathBuf,
    pub major_version: String,
}

#[async_trait]
pub trait ChromeHost: Send + Sync {
    async fn discover_chrome(&self) -> Result<DetectedChrome, ChromeToolError>;

    async fn open_session(
        &self,
        url: &str,
        browser_binary: &Path,
        driver_binary: &Path,
    ) -> Result<BackendOpenResult, ChromeToolError>;
}

struct ManagedChromeBackend {
    host: Arc<dyn ChromeHost>,
    installer: Arc<ChromeInstaller>,
}

impl ManagedChromeBackend {
    fn new(host: Arc<dyn ChromeHost>, installer: Arc<ChromeInstaller>) -> Self {
        Self { host, installer }
    }
}

#[async_trait]
impl BrowserBackend for ManagedChromeBackend {
    async fn open(&self, url: &str) -> Result<BackendOpenResult, ChromeToolError> {
        let detected = self.host.discover_chrome().await?;
        let install = self
            .installer
            .ensure_driver(&detected.major_version)
            .await?;
        self.host
            .open_session(url, &detected.browser_binary, &install.patched_driver)
            .await
    }
}

pub struct ChromeManager {
    backend: Arc<dyn BrowserBackend>,
    paths: ChromePaths,
    sessions: RwLock<HashMap<String, ChromeSession>>,
    session_order: RwLock<VecDeque<String>>,
    session_limit: usize,
    next_session_id: AtomicU64,
    next_screenshot_id: AtomicU64,
}

impl ChromeManager {
    #[must_use]
    pub fn new(backend: Arc<dyn BrowserBackend>, paths: ChromePaths) -> Self {
        Self::new_with_session_limit(backend, paths, 4)
    }

    #[must_use]
    fn new_with_session_limit(
        backend: Arc<dyn BrowserBackend>,
        paths: ChromePaths,
        session_limit: usize,
    ) -> Self {
        Self {
            backend,
            paths,
            sessions: RwLock::new(HashMap::new()),
            session_order: RwLock::new(VecDeque::new()),
            session_limit,
            next_session_id: AtomicU64::new(0),
            next_screenshot_id: AtomicU64::new(0),
        }
    }

    #[must_use]
    pub fn new_production(paths: ChromePaths) -> Self {
        let host: Arc<dyn ChromeHost> = Arc::new(SystemChromeHost);
        let downloader: Arc<dyn DriverDownloader> = Arc::new(ReqwestDriverDownloader::new());
        let installer = Arc::new(ChromeInstaller::new(paths.clone(), downloader));
        let backend: Arc<dyn BrowserBackend> = Arc::new(ManagedChromeBackend::new(host, installer));
        Self::new(backend, paths)
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn new_with_managed_components_for_test(
        host: Arc<dyn ChromeHost>,
        downloader: Arc<dyn DriverDownloader>,
        paths: ChromePaths,
    ) -> Self {
        let installer = Arc::new(ChromeInstaller::new(paths.clone(), downloader));
        let backend: Arc<dyn BrowserBackend> = Arc::new(ManagedChromeBackend::new(host, installer));
        Self::new(backend, paths)
    }

    #[cfg(test)]
    #[must_use]
    pub fn new_for_test(backend: Arc<dyn BrowserBackend>) -> Self {
        static NEXT_TEST_MANAGER_ID: AtomicU64 = AtomicU64::new(0);
        let id = NEXT_TEST_MANAGER_ID.fetch_add(1, Ordering::Relaxed) + 1;
        let home = std::env::temp_dir().join(format!("arguswing-chrome-tests-{id}"));
        Self::new_with_session_limit(backend, ChromePaths::from_home(&home), 4)
    }

    pub async fn open(&self, args: OpenArgs) -> Result<OpenedSession, ChromeToolError> {
        self.paths.ensure_directories()?;
        let opened = self.backend.open(&args.url).await?;
        let session_id = self.next_session_id();

        let session = ChromeSession::new(
            session_id.clone(),
            opened.metadata.final_url.clone(),
            opened.metadata.page_title.clone(),
            opened.session,
        );
        self.sessions
            .write()
            .await
            .insert(session_id.clone(), session);
        self.session_order
            .write()
            .await
            .push_back(session_id.clone());
        self.evict_excess_sessions().await?;

        Ok(OpenedSession {
            session_id,
            final_url: opened.metadata.final_url,
            page_title: opened.metadata.page_title,
        })
    }

    pub async fn close_session(&self, session_id: &str) -> Result<(), ChromeToolError> {
        self.remove_session_order_entry(session_id).await;
        let session = self
            .sessions
            .write()
            .await
            .remove(session_id)
            .ok_or_else(|| Self::session_not_found(session_id))?;
        session.interaction().shutdown().await
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
        self.session_interaction(session_id)
            .await?
            .list_links()
            .await
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
        screenshot_name: Option<&str>,
    ) -> Result<PathBuf, ChromeToolError> {
        self.paths.ensure_directories()?;
        let interaction = self.session_interaction(session_id).await?;
        let screenshot_path = self.managed_screenshot_path(session_id, screenshot_name)?;
        let png = interaction.screenshot_png().await?;
        std::fs::write(&screenshot_path, png).map_err(|e| ChromeToolError::FileWriteFailed {
            path: screenshot_path.clone(),
            reason: e.to_string(),
        })?;

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

    fn managed_screenshot_path(
        &self,
        session_id: &str,
        screenshot_name: Option<&str>,
    ) -> Result<PathBuf, ChromeToolError> {
        if let Some(name) = screenshot_name {
            let candidate = Path::new(name);
            let is_file_name_only =
                candidate.file_name().and_then(|value| value.to_str()) == Some(name);
            let is_png = candidate
                .extension()
                .and_then(|value| value.to_str())
                .is_some_and(|value| value.eq_ignore_ascii_case("png"));
            if !is_file_name_only || !is_png {
                return Err(ChromeToolError::OutputPathNotAllowed {
                    path: name.to_string(),
                });
            }
            return Ok(self.paths.screenshots.join(name));
        }

        let next = self.next_screenshot_id.fetch_add(1, Ordering::Relaxed) + 1;
        Ok(self
            .paths
            .screenshots
            .join(format!("{session_id}-{next}.png")))
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

    async fn evict_excess_sessions(&self) -> Result<(), ChromeToolError> {
        loop {
            let evicted_session_id = {
                let mut order = self.session_order.write().await;
                if order.len() <= self.session_limit {
                    return Ok(());
                }
                order.pop_front()
            };

            let Some(session_id) = evicted_session_id else {
                return Ok(());
            };

            self.close_session(&session_id).await?;
        }
    }

    async fn remove_session_order_entry(&self, session_id: &str) {
        let mut order = self.session_order.write().await;
        if let Some(index) = order.iter().position(|value| value == session_id) {
            order.remove(index);
        }
    }

    fn session_not_found(session_id: &str) -> ChromeToolError {
        ChromeToolError::SessionNotFound {
            session_id: session_id.to_string(),
        }
    }
}

impl Drop for ChromeManager {
    fn drop(&mut self) {
        let sessions = std::mem::take(self.sessions.get_mut());
        self.session_order.get_mut().clear();
        if sessions.is_empty() {
            return;
        }

        let interactions: Vec<_> = sessions
            .into_values()
            .map(|session| session.interaction())
            .collect();
        let _ = std::thread::spawn(move || {
            if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                runtime.block_on(async move {
                    for interaction in interactions {
                        let _ = interaction.shutdown().await;
                    }
                });
            }
        })
        .join();
    }
}

#[derive(Debug, Default)]
pub struct SystemChromeHost;

#[async_trait]
impl ChromeHost for SystemChromeHost {
    async fn discover_chrome(&self) -> Result<DetectedChrome, ChromeToolError> {
        let browser_binary = find_chrome_binary()?;
        let output = Command::new(&browser_binary)
            .arg("--version")
            .output()
            .await
            .map_err(|e| ChromeToolError::ChromeVersionDetectFailed {
                path: browser_binary.clone(),
                reason: e.to_string(),
            })?;
        if !output.status.success() {
            let reason = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(ChromeToolError::ChromeVersionDetectFailed {
                path: browser_binary.clone(),
                reason: if reason.is_empty() {
                    format!("chrome exited with status {}", output.status)
                } else {
                    reason
                },
            });
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let major_version = parse_major_version(&stdout).ok_or_else(|| {
            ChromeToolError::ChromeVersionDetectFailed {
                path: browser_binary.clone(),
                reason: format!("unexpected version output: {stdout}"),
            }
        })?;

        Ok(DetectedChrome {
            browser_binary,
            major_version,
        })
    }

    async fn open_session(
        &self,
        url: &str,
        browser_binary: &Path,
        driver_binary: &Path,
    ) -> Result<BackendOpenResult, ChromeToolError> {
        let port = reserve_free_port()?;
        let server_url = format!("http://127.0.0.1:{port}");
        let mut child = Command::new(driver_binary)
            .arg(format!("--port={port}"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| ChromeToolError::DriverStartFailed {
                reason: e.to_string(),
            })?;

        if let Err(err) = wait_for_driver_ready(&mut child, port).await {
            let _ = shutdown_child_process(&mut child).await;
            return Err(err);
        }

        let mut capabilities = DesiredCapabilities::chrome();
        capabilities
            .set_binary(browser_binary.to_string_lossy().as_ref())
            .map_err(|e| ChromeToolError::DriverStartFailed {
                reason: e.to_string(),
            })?;
        capabilities
            .add_arg("--headless=new")
            .map_err(|e| ChromeToolError::DriverStartFailed {
                reason: e.to_string(),
            })?;
        capabilities
            .add_arg("--disable-gpu")
            .map_err(|e| ChromeToolError::DriverStartFailed {
                reason: e.to_string(),
            })?;
        capabilities
            .add_arg("--no-first-run")
            .map_err(|e| ChromeToolError::DriverStartFailed {
                reason: e.to_string(),
            })?;
        capabilities
            .add_arg("--no-default-browser-check")
            .map_err(|e| ChromeToolError::DriverStartFailed {
                reason: e.to_string(),
            })?;

        let driver = match WebDriver::new(&server_url, capabilities).await {
            Ok(driver) => driver,
            Err(err) => {
                let _ = shutdown_child_process(&mut child).await;
                return Err(ChromeToolError::DriverStartFailed {
                    reason: err.to_string(),
                });
            }
        };

        if let Err(err) = driver.goto(url).await {
            let _ = driver.clone().quit().await;
            let _ = shutdown_child_process(&mut child).await;
            return Err(ChromeToolError::NavigationFailed {
                url: url.to_string(),
                reason: err.to_string(),
            });
        }
        let final_url = match driver.current_url().await {
            Ok(value) => value.to_string(),
            Err(err) => {
                let _ = driver.clone().quit().await;
                let _ = shutdown_child_process(&mut child).await;
                return Err(ChromeToolError::PageReadFailed {
                    reason: err.to_string(),
                });
            }
        };
        let page_title = match driver.title().await {
            Ok(value) => value,
            Err(err) => {
                let _ = driver.clone().quit().await;
                let _ = shutdown_child_process(&mut child).await;
                return Err(ChromeToolError::PageReadFailed {
                    reason: err.to_string(),
                });
            }
        };

        let session: Arc<dyn BrowserSession> =
            Arc::new(ManagedWebDriverSession::new(driver, child));
        Ok(BackendOpenResult {
            metadata: PageMetadata {
                final_url,
                page_title,
            },
            session,
        })
    }
}

fn parse_major_version(output: &str) -> Option<String> {
    output
        .split_whitespace()
        .find_map(|token| {
            token
                .split('.')
                .next()
                .filter(|value| value.chars().all(|ch| ch.is_ascii_digit()))
        })
        .map(str::to_string)
}

fn reserve_free_port() -> Result<u16, ChromeToolError> {
    let listener =
        TcpListener::bind("127.0.0.1:0").map_err(|e| ChromeToolError::DriverStartFailed {
            reason: e.to_string(),
        })?;
    listener
        .local_addr()
        .map(|address| address.port())
        .map_err(|e| ChromeToolError::DriverStartFailed {
            reason: e.to_string(),
        })
}

async fn wait_for_driver_ready(
    child: &mut tokio::process::Child,
    port: u16,
) -> Result<(), ChromeToolError> {
    let address: SocketAddr = format!("127.0.0.1:{port}")
        .parse::<SocketAddr>()
        .map_err(|e| ChromeToolError::DriverStartFailed {
            reason: e.to_string(),
        })?;
    for _ in 0..50 {
        if let Some(status) = child
            .try_wait()
            .map_err(|e| ChromeToolError::DriverStartFailed {
                reason: e.to_string(),
            })?
        {
            return Err(ChromeToolError::DriverStartFailed {
                reason: format!("chromedriver exited before accepting connections: {status}"),
            });
        }
        if TcpStream::connect_timeout(&address, Duration::from_millis(50)).is_ok() {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    Err(ChromeToolError::DriverStartFailed {
        reason: format!("timed out waiting for chromedriver on port {port}"),
    })
}

fn find_chrome_binary() -> Result<PathBuf, ChromeToolError> {
    if let Some(path) = std::env::var_os("ARGUS_CHROME_BINARY").map(PathBuf::from) {
        if path.is_file() {
            return Ok(path);
        }
    }
    if let Some(path) = std::env::var_os("CHROME_BINARY").map(PathBuf::from) {
        if path.is_file() {
            return Ok(path);
        }
    }

    chrome_binary_candidates()
        .into_iter()
        .find(|path| path.is_file())
        .ok_or(ChromeToolError::ChromeNotInstalled)
}

fn chrome_binary_candidates() -> Vec<PathBuf> {
    match std::env::consts::OS {
        "macos" => vec![
            PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            PathBuf::from("/Applications/Chromium.app/Contents/MacOS/Chromium"),
        ],
        "linux" => vec![
            PathBuf::from("/usr/bin/google-chrome"),
            PathBuf::from("/usr/bin/google-chrome-stable"),
            PathBuf::from("/usr/bin/chromium"),
            PathBuf::from("/usr/bin/chromium-browser"),
        ],
        "windows" => {
            let mut candidates = Vec::new();
            for key in ["PROGRAMFILES", "PROGRAMFILES(X86)", "LOCALAPPDATA"] {
                if let Some(root) = std::env::var_os(key) {
                    let root = PathBuf::from(root);
                    candidates.push(
                        root.join("Google")
                            .join("Chrome")
                            .join("Application")
                            .join("chrome.exe"),
                    );
                }
            }
            candidates
        }
        _ => Vec::new(),
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::sync::Arc;
    use std::sync::Mutex as StdMutex;

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
        screenshot: Vec<u8>,
        shutdown_label: String,
    }

    #[derive(Debug, Default)]
    struct FakeBrowserBackend {
        pages: HashMap<String, FakePage>,
        shutdowns: Arc<StdMutex<Vec<String>>>,
    }

    impl FakeBrowserBackend {
        fn with_shutdowns(mut self, shutdowns: Arc<StdMutex<Vec<String>>>) -> Self {
            self.shutdowns = shutdowns;
            self
        }

        fn with_page(
            mut self,
            requested_url: impl Into<String>,
            final_url: impl Into<String>,
            page_title: impl Into<String>,
            links: Vec<LinkSummary>,
            text: impl Into<String>,
        ) -> Self {
            let requested_url = requested_url.into();
            self.pages.insert(
                requested_url.clone(),
                FakePage {
                    final_url: final_url.into(),
                    page_title: page_title.into(),
                    links,
                    text: text.into(),
                    screenshot: b"fake-png".to_vec(),
                    shutdown_label: requested_url,
                },
            );
            self
        }
    }

    #[derive(Debug)]
    struct FakeBrowserSession {
        links: Vec<LinkSummary>,
        text: String,
        screenshot: Vec<u8>,
        shutdown_label: String,
        shutdowns: Arc<StdMutex<Vec<String>>>,
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
            self.shutdowns
                .lock()
                .unwrap()
                .push(self.shutdown_label.clone());
            Ok(())
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
                screenshot: page.screenshot.clone(),
                shutdown_label: page.shutdown_label.clone(),
                shutdowns: Arc::clone(&self.shutdowns),
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
    async fn manager_evicts_oldest_session_and_shuts_it_down_when_capacity_is_exceeded() {
        let shutdowns = Arc::new(StdMutex::new(Vec::new()));
        let manager = ChromeManager::new_for_test(Arc::new(
            FakeBrowserBackend::default()
                .with_shutdowns(Arc::clone(&shutdowns))
                .with_page(
                    "https://example.com/1",
                    "https://example.com/1",
                    "One",
                    Vec::new(),
                    "One",
                )
                .with_page(
                    "https://example.com/2",
                    "https://example.com/2",
                    "Two",
                    Vec::new(),
                    "Two",
                )
                .with_page(
                    "https://example.com/3",
                    "https://example.com/3",
                    "Three",
                    Vec::new(),
                    "Three",
                )
                .with_page(
                    "https://example.com/4",
                    "https://example.com/4",
                    "Four",
                    Vec::new(),
                    "Four",
                )
                .with_page(
                    "https://example.com/5",
                    "https://example.com/5",
                    "Five",
                    Vec::new(),
                    "Five",
                ),
        ));

        let mut opened = Vec::new();
        for index in 1..=5 {
            opened.push(
                manager
                    .open(OpenArgs {
                        url: format!("https://example.com/{index}"),
                    })
                    .await
                    .unwrap(),
            );
        }

        let err = manager.session(&opened[0].session_id).await.unwrap_err();
        assert!(matches!(err, ChromeToolError::SessionNotFound { .. }));
        assert_eq!(
            shutdowns.lock().unwrap().as_slice(),
            &["https://example.com/1".to_string()]
        );
    }

    #[test]
    fn manager_drop_shuts_down_live_sessions() {
        let shutdowns = Arc::new(StdMutex::new(Vec::new()));
        let manager = ChromeManager::new_for_test(Arc::new(
            FakeBrowserBackend::default()
                .with_shutdowns(Arc::clone(&shutdowns))
                .with_page(
                    "https://example.com/drop",
                    "https://example.com/drop",
                    "Drop",
                    Vec::new(),
                    "Drop",
                ),
        ));
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        runtime.block_on(async {
            manager
                .open(OpenArgs {
                    url: "https://example.com/drop".to_string(),
                })
                .await
                .unwrap();
        });
        drop(runtime);
        drop(manager);

        assert_eq!(
            shutdowns.lock().unwrap().as_slice(),
            &["https://example.com/drop".to_string()]
        );
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

        let returned = manager.screenshot(&opened.session_id, None).await.unwrap();
        assert!(returned.starts_with(&manager.paths.screenshots));
        assert!(returned.is_file());

        let session = manager.session(&opened.session_id).await.unwrap();
        assert_eq!(session.last_screenshot_path, Some(returned));
    }

    #[tokio::test]
    async fn screenshot_rejects_arbitrary_output_path() {
        let manager = ChromeManager::new_for_test(sample_backend());
        let opened = manager
            .open(OpenArgs {
                url: "https://example.com".into(),
            })
            .await
            .unwrap();

        let err = manager
            .screenshot(&opened.session_id, Some("../../escape.png"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not allowed"));
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
            .screenshot("missing", Some("missing.png"))
            .await
            .unwrap_err();
        assert!(matches!(
            err,
            ChromeToolError::SessionNotFound { session_id } if session_id == "missing"
        ));
    }
}
