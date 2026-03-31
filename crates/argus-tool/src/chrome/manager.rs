use std::collections::{HashMap, VecDeque};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use async_trait::async_trait;
use thirtyfour::common::capabilities::chrome::ChromeCapabilities;
use thirtyfour::common::capabilities::desiredcapabilities::{CapabilitiesHelper, PageLoadStrategy};
use thirtyfour::prelude::{ChromiumLikeCapabilities, DesiredCapabilities, WebDriver};
use tokio::process::Command;
use tokio::sync::RwLock;

use super::error::ChromeToolError;
use super::installer::{
    ChromeInstaller, ChromePaths, DriverDownloader, InstalledDriver, ReqwestDriverDownloader,
};
use super::models::{
    CookieSummary, LinkSummary, NetworkRequestSummary, OpenArgs, OpenedSession, PageMetadata,
};
use super::session::{
    BrowserSession, ChromeSession, ManagedWebDriverSession, install_network_request_recorder,
    shutdown_child_process,
};

pub struct BackendOpenResult {
    pub metadata: PageMetadata,
    pub session: Arc<dyn BrowserSession>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SessionMode {
    Readonly,
    Interactive,
}

#[async_trait]
pub trait BrowserBackend: Send + Sync {
    async fn open(&self, url: &str) -> Result<BackendOpenResult, ChromeToolError>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedChrome {
    pub browser_binary: PathBuf,
    pub browser_version: String,
}

#[async_trait]
pub trait ChromeHost: Send + Sync {
    async fn discover_chrome(&self) -> Result<DetectedChrome, ChromeToolError>;

    async fn open_session(
        &self,
        url: &str,
        browser_binary: &Path,
        browser_version: &str,
        driver_binary: &Path,
        session_mode: SessionMode,
    ) -> Result<BackendOpenResult, ChromeToolError>;
}

struct ManagedChromeBackend {
    host: Arc<dyn ChromeHost>,
    installer: Arc<ChromeInstaller>,
    session_mode: SessionMode,
}

#[derive(Clone)]
struct ManagedChromeSupport {
    host: Arc<dyn ChromeHost>,
    installer: Arc<ChromeInstaller>,
}

impl ManagedChromeBackend {
    fn new(
        host: Arc<dyn ChromeHost>,
        installer: Arc<ChromeInstaller>,
        session_mode: SessionMode,
    ) -> Self {
        Self {
            host,
            installer,
            session_mode,
        }
    }
}

#[async_trait]
impl BrowserBackend for ManagedChromeBackend {
    async fn open(&self, url: &str) -> Result<BackendOpenResult, ChromeToolError> {
        let detected = self.host.discover_chrome().await?;
        let install = self
            .installer
            .find_installed_driver(&detected.browser_version)?
            .ok_or_else(|| ChromeToolError::DriverNotInstalled {
                browser_version: detected.browser_version.clone(),
                suggested_action: "install".to_string(),
            })?;
        self.host
            .open_session(
                url,
                &detected.browser_binary,
                &detected.browser_version,
                &install.patched_driver,
                self.session_mode,
            )
            .await
    }
}

pub struct ChromeManager {
    backend: Arc<dyn BrowserBackend>,
    managed_support: Option<ManagedChromeSupport>,
    paths: ChromePaths,
    sessions: RwLock<HashMap<String, ChromeSession>>,
    session_order: RwLock<VecDeque<String>>,
    session_limit: usize,
    next_session_id: AtomicU64,
}

impl ChromeManager {
    #[allow(dead_code)]
    const DEFAULT_SESSION_LIMIT: usize = 4;
    const PRODUCTION_SESSION_LIMIT: usize = 1;

    #[must_use]
    #[allow(dead_code)]
    pub fn new(backend: Arc<dyn BrowserBackend>, paths: ChromePaths) -> Self {
        Self::new_with_session_limit(backend, None, paths, Self::DEFAULT_SESSION_LIMIT)
    }

    #[must_use]
    fn new_with_session_limit(
        backend: Arc<dyn BrowserBackend>,
        managed_support: Option<ManagedChromeSupport>,
        paths: ChromePaths,
        session_limit: usize,
    ) -> Self {
        Self {
            backend,
            managed_support,
            paths,
            sessions: RwLock::new(HashMap::new()),
            session_order: RwLock::new(VecDeque::new()),
            session_limit,
            next_session_id: AtomicU64::new(0),
        }
    }

    #[must_use]
    pub fn new_production(paths: ChromePaths) -> Self {
        let host: Arc<dyn ChromeHost> = Arc::new(SystemChromeHost);
        let downloader: Arc<dyn DriverDownloader> = Arc::new(ReqwestDriverDownloader::new());
        let installer = Arc::new(ChromeInstaller::new(paths.clone(), downloader));
        let managed_support = Some(ManagedChromeSupport {
            host: host.clone(),
            installer: installer.clone(),
        });
        let backend: Arc<dyn BrowserBackend> = Arc::new(ManagedChromeBackend::new(
            host,
            installer,
            SessionMode::Readonly,
        ));
        Self::new_with_session_limit(
            backend,
            managed_support,
            paths,
            Self::PRODUCTION_SESSION_LIMIT,
        )
    }

    #[must_use]
    pub fn new_interactive_production(paths: ChromePaths) -> Self {
        let host: Arc<dyn ChromeHost> = Arc::new(SystemChromeHost);
        let downloader: Arc<dyn DriverDownloader> = Arc::new(ReqwestDriverDownloader::new());
        let installer = Arc::new(ChromeInstaller::new(paths.clone(), downloader));
        let managed_support = Some(ManagedChromeSupport {
            host: host.clone(),
            installer: installer.clone(),
        });
        let backend: Arc<dyn BrowserBackend> = Arc::new(ManagedChromeBackend::new(
            host,
            installer,
            SessionMode::Interactive,
        ));
        Self::new_with_session_limit(
            backend,
            managed_support,
            paths,
            Self::PRODUCTION_SESSION_LIMIT,
        )
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn new_with_managed_components_for_test(
        host: Arc<dyn ChromeHost>,
        downloader: Arc<dyn DriverDownloader>,
        paths: ChromePaths,
    ) -> Self {
        let installer = Arc::new(ChromeInstaller::new(paths.clone(), downloader));
        let managed_support = Some(ManagedChromeSupport {
            host: host.clone(),
            installer: installer.clone(),
        });
        let backend: Arc<dyn BrowserBackend> = Arc::new(ManagedChromeBackend::new(
            host,
            installer,
            SessionMode::Readonly,
        ));
        Self::new_with_session_limit(
            backend,
            managed_support,
            paths,
            Self::PRODUCTION_SESSION_LIMIT,
        )
    }

    #[cfg(test)]
    #[must_use]
    pub(crate) fn new_interactive_with_managed_components_for_test(
        host: Arc<dyn ChromeHost>,
        downloader: Arc<dyn DriverDownloader>,
        paths: ChromePaths,
    ) -> Self {
        let installer = Arc::new(ChromeInstaller::new(paths.clone(), downloader));
        let managed_support = Some(ManagedChromeSupport {
            host: host.clone(),
            installer: installer.clone(),
        });
        let backend: Arc<dyn BrowserBackend> = Arc::new(ManagedChromeBackend::new(
            host,
            installer,
            SessionMode::Interactive,
        ));
        Self::new_with_session_limit(
            backend,
            managed_support,
            paths,
            Self::PRODUCTION_SESSION_LIMIT,
        )
    }

    #[cfg(test)]
    #[must_use]
    pub fn new_for_test(backend: Arc<dyn BrowserBackend>) -> Self {
        static NEXT_TEST_MANAGER_ID: AtomicU64 = AtomicU64::new(0);
        let id = NEXT_TEST_MANAGER_ID.fetch_add(1, Ordering::Relaxed) + 1;
        let home = std::env::temp_dir().join(format!("arguswing-chrome-tests-{id}"));
        Self::new(backend, ChromePaths::from_home(&home))
    }

    pub async fn install_driver(
        &self,
    ) -> Result<(DetectedChrome, InstalledDriver), ChromeToolError> {
        let support = self
            .managed_support
            .as_ref()
            .ok_or(ChromeToolError::InstallUnavailable)?;
        let detected = support.host.discover_chrome().await?;
        let install = support
            .installer
            .ensure_driver(&detected.browser_version)
            .await?;
        Ok((detected, install))
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

    pub async fn click(&self, session_id: &str, selector: &str) -> Result<(), ChromeToolError> {
        self.session_interaction(session_id)
            .await?
            .click(selector)
            .await
    }

    pub async fn type_text(
        &self,
        session_id: &str,
        selector: &str,
        text: &str,
    ) -> Result<(), ChromeToolError> {
        self.session_interaction(session_id)
            .await?
            .type_text(selector, text)
            .await
    }

    pub async fn current_url(&self, session_id: &str) -> Result<String, ChromeToolError> {
        self.session_interaction(session_id)
            .await?
            .current_url()
            .await
    }

    pub async fn get_cookies(
        &self,
        session_id: &str,
    ) -> Result<Vec<CookieSummary>, ChromeToolError> {
        self.session_interaction(session_id)
            .await?
            .get_cookies()
            .await
    }

    pub async fn network_requests(
        &self,
        session_id: &str,
        max_requests: Option<u32>,
    ) -> Result<Vec<NetworkRequestSummary>, ChromeToolError> {
        self.session_interaction(session_id)
            .await?
            .network_requests(max_requests)
            .await
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
        let browser_version = detect_browser_version(&browser_binary).await?;

        Ok(DetectedChrome {
            browser_binary,
            browser_version,
        })
    }

    async fn open_session(
        &self,
        url: &str,
        browser_binary: &Path,
        browser_version: &str,
        driver_binary: &Path,
        session_mode: SessionMode,
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

        let capabilities =
            build_chrome_capabilities(browser_binary, browser_version, session_mode)?;

        let driver = match WebDriver::new(&server_url, capabilities).await {
            Ok(driver) => driver,
            Err(err) => {
                let _ = shutdown_child_process(&mut child).await;
                return Err(ChromeToolError::DriverStartFailed {
                    reason: err.to_string(),
                });
            }
        };

        if let Err(err) = install_network_request_recorder(&driver).await {
            let _ = driver.clone().quit().await;
            let _ = shutdown_child_process(&mut child).await;
            return Err(err);
        }

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

async fn detect_browser_version(browser_binary: &Path) -> Result<String, ChromeToolError> {
    let output = chrome_version_command_output(browser_binary).await?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        return Err(ChromeToolError::ChromeVersionDetectFailed {
            path: browser_binary.to_path_buf(),
            reason: if !stderr.is_empty() {
                stderr
            } else if !stdout.is_empty() {
                stdout
            } else {
                format!("chrome exited with status {}", output.status)
            },
        });
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    parse_browser_version_output(&stdout).ok_or_else(|| {
        ChromeToolError::ChromeVersionDetectFailed {
            path: browser_binary.to_path_buf(),
            reason: format!("unexpected version output: {stdout}"),
        }
    })
}

async fn chrome_version_command_output(
    browser_binary: &Path,
) -> Result<std::process::Output, ChromeToolError> {
    let mut command = if std::env::consts::OS == "windows" {
        let escaped_path = browser_binary.to_string_lossy().replace('\'', "''");
        let mut command = Command::new("powershell");
        command.arg("-NoProfile").arg("-Command").arg(format!(
            "(Get-Item '{}').VersionInfo.ProductVersion",
            escaped_path
        ));
        command
    } else {
        let mut command = Command::new(browser_binary);
        command.arg("--version");
        command
    };

    command
        .output()
        .await
        .map_err(|e| ChromeToolError::ChromeVersionDetectFailed {
            path: browser_binary.to_path_buf(),
            reason: e.to_string(),
        })
}

fn parse_browser_version_output(output: &str) -> Option<String> {
    output
        .split(|ch: char| !(ch.is_ascii_digit() || ch == '.'))
        .filter(|token| !token.is_empty())
        .find(|token| token.chars().all(|ch| ch.is_ascii_digit() || ch == '.'))
        .map(str::to_string)
}

fn build_chrome_capabilities(
    browser_binary: &Path,
    browser_version: &str,
    session_mode: SessionMode,
) -> Result<ChromeCapabilities, ChromeToolError> {
    let mut capabilities = DesiredCapabilities::chrome();
    capabilities
        .set_binary(browser_binary.to_string_lossy().as_ref())
        .map_err(|e| ChromeToolError::DriverStartFailed {
            reason: e.to_string(),
        })?;

    match session_mode {
        SessionMode::Readonly => {
            capabilities.add_arg("--headless=new").map_err(|e| {
                ChromeToolError::DriverStartFailed {
                    reason: e.to_string(),
                }
            })?;
            capabilities.add_arg("--disable-gpu").map_err(|e| {
                ChromeToolError::DriverStartFailed {
                    reason: e.to_string(),
                }
            })?;
            capabilities.add_arg("--no-first-run").map_err(|e| {
                ChromeToolError::DriverStartFailed {
                    reason: e.to_string(),
                }
            })?;
            capabilities
                .add_arg("--no-default-browser-check")
                .map_err(|e| ChromeToolError::DriverStartFailed {
                    reason: e.to_string(),
                })?;
        }
        SessionMode::Interactive => {
            let user_agent_arg = format!("user-agent={}", interactive_user_agent(browser_version));
            capabilities
                .set_page_load_strategy(PageLoadStrategy::Eager)
                .map_err(|e| ChromeToolError::DriverStartFailed {
                    reason: e.to_string(),
                })?;
            capabilities
                .set_no_sandbox()
                .map_err(|e| ChromeToolError::DriverStartFailed {
                    reason: e.to_string(),
                })?;
            capabilities.set_disable_dev_shm_usage().map_err(|e| {
                ChromeToolError::DriverStartFailed {
                    reason: e.to_string(),
                }
            })?;
            capabilities
                .add_arg("--disable-blink-features=AutomationControlled")
                .map_err(|e| ChromeToolError::DriverStartFailed {
                    reason: e.to_string(),
                })?;
            capabilities.add_arg("window-size=1920,1080").map_err(|e| {
                ChromeToolError::DriverStartFailed {
                    reason: e.to_string(),
                }
            })?;
            capabilities.add_arg(&user_agent_arg).map_err(|e| {
                ChromeToolError::DriverStartFailed {
                    reason: e.to_string(),
                }
            })?;
            capabilities.add_arg("--disable-infobars").map_err(|e| {
                ChromeToolError::DriverStartFailed {
                    reason: e.to_string(),
                }
            })?;
            capabilities
                .add_experimental_option("excludeSwitches", vec!["enable-automation"])
                .map_err(|e| ChromeToolError::DriverStartFailed {
                    reason: e.to_string(),
                })?;
            capabilities.add_arg("--no-first-run").map_err(|e| {
                ChromeToolError::DriverStartFailed {
                    reason: e.to_string(),
                }
            })?;
            capabilities
                .add_arg("--no-default-browser-check")
                .map_err(|e| ChromeToolError::DriverStartFailed {
                    reason: e.to_string(),
                })?;
        }
    }

    Ok(capabilities)
}

fn interactive_user_agent(browser_version: &str) -> String {
    interactive_user_agent_for_os(std::env::consts::OS, browser_version)
}

fn interactive_user_agent_for_os(os: &str, browser_version: &str) -> String {
    let platform = match os {
        "macos" => "Macintosh; Intel Mac OS X 10_15_7",
        "windows" => "Windows NT 10.0; Win64; x64",
        _ => "X11; Linux x86_64",
    };
    format!(
        "Mozilla/5.0 ({platform}) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/{browser_version} Safari/537.36"
    )
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
    if let Some(path) = std::env::var_os("ARGUS_CHROME_BINARY").map(PathBuf::from)
        && path.is_file()
    {
        return Ok(path);
    }
    if let Some(path) = std::env::var_os("CHROME_BINARY").map(PathBuf::from)
        && path.is_file()
    {
        return Ok(path);
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
    use std::path::Path;
    use std::sync::Arc;
    use std::sync::Mutex as StdMutex;

    use serde_json::json;

    use crate::chrome::error::ChromeToolError;
    use crate::chrome::models::{
        CookieSummary, LinkSummary, NetworkRequestSummary, OpenArgs, PageMetadata,
    };
    use crate::chrome::session::BrowserSession;

    use super::{
        BackendOpenResult, BrowserBackend, ChromeManager, SessionMode, build_chrome_capabilities,
        interactive_user_agent_for_os, parse_browser_version_output,
    };

    #[derive(Debug, Clone)]
    struct FakePage {
        final_url: String,
        page_title: String,
        links: Vec<LinkSummary>,
        text: String,
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
        shutdown_label: String,
        shutdowns: Arc<StdMutex<Vec<String>>>,
        url: String,
        cookies: Vec<CookieSummary>,
        network_requests: Vec<NetworkRequestSummary>,
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

        async fn shutdown(&self) -> Result<(), ChromeToolError> {
            self.shutdowns
                .lock()
                .unwrap()
                .push(self.shutdown_label.clone());
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

        async fn network_requests(
            &self,
            _max_requests: Option<u32>,
        ) -> Result<Vec<NetworkRequestSummary>, ChromeToolError> {
            Ok(self.network_requests.clone())
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
                shutdown_label: page.shutdown_label.clone(),
                shutdowns: Arc::clone(&self.shutdowns),
                url: page.final_url.clone(),
                cookies: vec![],
                network_requests: vec![],
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
    }

    #[test]
    fn parse_browser_version_output_supports_macos_format() {
        assert_eq!(
            parse_browser_version_output("Google Chrome 124.0.6367.91"),
            Some("124.0.6367.91".to_string())
        );
    }

    #[test]
    fn parse_browser_version_output_supports_windows_format() {
        assert_eq!(
            parse_browser_version_output("ProductVersion\r\n124.0.6367.91\r\n"),
            Some("124.0.6367.91".to_string())
        );
    }

    #[test]
    fn interactive_capabilities_use_visible_browser_arguments() {
        let caps = build_chrome_capabilities(
            Path::new("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            "124.0.6367.91",
            SessionMode::Interactive,
        )
        .unwrap();
        let caps_json = serde_json::to_value(caps).unwrap();
        let args = caps_json["goog:chromeOptions"]["args"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|value| value.as_str())
            .collect::<Vec<_>>();

        assert!(!args.contains(&"--headless=new"));
        assert!(args.contains(&"--disable-blink-features=AutomationControlled"));
        assert!(args.contains(&"window-size=1920,1080"));
        assert!(args.iter().any(|value| value.starts_with("user-agent=")));
        assert!(args.contains(&"--disable-infobars"));
        assert_eq!(
            caps_json["goog:chromeOptions"]["excludeSwitches"],
            json!(["enable-automation"])
        );
    }

    #[test]
    fn interactive_capabilities_use_detected_browser_version_in_user_agent() {
        let caps = build_chrome_capabilities(
            Path::new("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            "124.0.6367.91",
            SessionMode::Interactive,
        )
        .unwrap();
        let caps_json = serde_json::to_value(caps).unwrap();
        let args = caps_json["goog:chromeOptions"]["args"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|value| value.as_str())
            .collect::<Vec<_>>();
        let user_agent = args
            .iter()
            .find(|value| value.starts_with("user-agent="))
            .copied()
            .unwrap();

        assert!(user_agent.contains("Chrome/124.0.6367.91"));
    }

    #[test]
    fn interactive_capabilities_use_eager_page_load_strategy() {
        let caps = build_chrome_capabilities(
            Path::new("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            "124.0.6367.91",
            SessionMode::Interactive,
        )
        .unwrap();
        let caps_json = serde_json::to_value(caps).unwrap();

        assert_eq!(caps_json["pageLoadStrategy"], json!("eager"));
    }

    #[test]
    fn readonly_capabilities_keep_headless_configuration() {
        let caps = build_chrome_capabilities(
            Path::new("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            "124.0.6367.91",
            SessionMode::Readonly,
        )
        .unwrap();
        let caps_json = serde_json::to_value(caps).unwrap();
        let args = caps_json["goog:chromeOptions"]["args"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|value| value.as_str())
            .collect::<Vec<_>>();

        assert!(args.contains(&"--headless=new"));
        assert!(!args.contains(&"--disable-blink-features=AutomationControlled"));
        assert!(caps_json["goog:chromeOptions"]["excludeSwitches"].is_null());
    }

    #[test]
    fn interactive_user_agent_uses_platform_specific_template() {
        assert!(
            interactive_user_agent_for_os("macos", "124.0.6367.91")
                .contains("Macintosh; Intel Mac OS X 10_15_7")
        );
        assert!(
            interactive_user_agent_for_os("windows", "124.0.6367.91")
                .contains("Windows NT 10.0; Win64; x64")
        );
    }
}
