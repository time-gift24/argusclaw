use std::future::Future;
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use thirtyfour::common::capabilities::chrome::ChromeCapabilities;
use thirtyfour::common::capabilities::desiredcapabilities::{CapabilitiesHelper, PageLoadStrategy};
use thirtyfour::common::cookie::Cookie;
use thirtyfour::prelude::{ChromiumLikeCapabilities, DesiredCapabilities, WebDriver};
use tokio::process::Command;
use tokio::sync::Mutex;

#[cfg(windows)]
use std::os::windows::process::CommandExt;

use super::error::ChromeToolError;
use super::installer::{
    ChromeInstaller, ChromePaths, DriverDownloader, InstalledDriver, ReqwestDriverDownloader,
};
use super::models::{NewTabResult, OpenedPage, PageMetadata, TabInfo};
use super::session::{BrowserSession, ChromeDriverProcess, ChromeSession, shutdown_child_process};

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
    shared_host: Option<Arc<SystemChromeHost>>,
}

const SHARED_DRIVER_PORT: u16 = 19_515;
#[cfg(any(test, windows))]
const WINDOWS_CREATE_NO_WINDOW: u32 = 0x0800_0000;

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
    session: Mutex<Option<ChromeSession>>,
}

impl ChromeManager {
    #[must_use]
    fn new_with_components(
        backend: Arc<dyn BrowserBackend>,
        managed_support: Option<ManagedChromeSupport>,
        paths: ChromePaths,
    ) -> Self {
        Self {
            backend,
            managed_support,
            paths,
            session: Mutex::new(None),
        }
    }

    #[must_use]
    pub fn new_production(paths: ChromePaths) -> Self {
        let shared_host = Arc::new(SystemChromeHost::default());
        let host: Arc<dyn ChromeHost> = shared_host.clone();
        let downloader: Arc<dyn DriverDownloader> = Arc::new(ReqwestDriverDownloader::new());
        let installer = Arc::new(ChromeInstaller::new(paths.clone(), downloader));
        let managed_support = Some(ManagedChromeSupport {
            host: host.clone(),
            installer: installer.clone(),
            shared_host: Some(shared_host),
        });
        let backend: Arc<dyn BrowserBackend> = Arc::new(ManagedChromeBackend::new(
            host,
            installer,
            SessionMode::Readonly,
        ));
        Self::new_with_components(backend, managed_support, paths)
    }

    #[must_use]
    pub fn new_interactive_production(paths: ChromePaths) -> Self {
        let shared_host = Arc::new(SystemChromeHost::default());
        let host: Arc<dyn ChromeHost> = shared_host.clone();
        let downloader: Arc<dyn DriverDownloader> = Arc::new(ReqwestDriverDownloader::new());
        let installer = Arc::new(ChromeInstaller::new(paths.clone(), downloader));
        let managed_support = Some(ManagedChromeSupport {
            host: host.clone(),
            installer: installer.clone(),
            shared_host: Some(shared_host),
        });
        let backend: Arc<dyn BrowserBackend> = Arc::new(ManagedChromeBackend::new(
            host,
            installer,
            SessionMode::Interactive,
        ));
        Self::new_with_components(backend, managed_support, paths)
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
            shared_host: None,
        });
        let backend: Arc<dyn BrowserBackend> = Arc::new(ManagedChromeBackend::new(
            host,
            installer,
            SessionMode::Readonly,
        ));
        Self::new_with_components(backend, managed_support, paths)
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
            shared_host: None,
        });
        let backend: Arc<dyn BrowserBackend> = Arc::new(ManagedChromeBackend::new(
            host,
            installer,
            SessionMode::Interactive,
        ));
        Self::new_with_components(backend, managed_support, paths)
    }

    #[cfg(test)]
    #[must_use]
    pub fn new_for_test(backend: Arc<dyn BrowserBackend>) -> Self {
        let home = tempfile::Builder::new()
            .prefix("arguswing-chrome-tests-")
            .tempdir()
            .map(|dir| dir.keep())
            .unwrap_or_else(|_| std::env::temp_dir().join("arguswing-chrome-tests"));
        Self::new_with_components(backend, None, ChromePaths::from_home(&home))
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

    pub async fn navigate(&self, url: &str) -> Result<OpenedPage, ChromeToolError> {
        self.paths.ensure_directories()?;

        let mut slot = self.session.lock().await;
        if let Some(session) = slot.as_mut() {
            let metadata = match session.interaction().navigate(url).await {
                Ok(metadata) => metadata,
                Err(error) if Self::is_stale_session_error(&error) => {
                    Self::shutdown_slot(&mut slot).await;
                    let session = self.open_new_session(url).await?;
                    let opened = opened_page_from_session(&session);
                    *slot = Some(session);
                    return Ok(opened);
                }
                Err(error) => return Err(error),
            };
            session.update_metadata(metadata.clone());
            return Ok(OpenedPage {
                final_url: metadata.final_url,
                page_title: metadata.page_title,
            });
        }

        let session = self.open_new_session(url).await?;
        let opened = opened_page_from_session(&session);
        *slot = Some(session);
        Ok(OpenedPage {
            final_url: opened.final_url,
            page_title: opened.page_title,
        })
    }

    pub async fn close(&self) -> Result<(), ChromeToolError> {
        let mut slot = self.session.lock().await;
        if let Some(session) = slot.take() {
            session.interaction().shutdown().await?;
        }
        Ok(())
    }

    pub async fn extract_text(&self, selector: Option<&str>) -> Result<String, ChromeToolError> {
        let mut slot = self.session.lock().await;
        let session = Self::active_session(&slot)?;
        let result = session.interaction().extract_text(selector).await;
        Self::clear_stale_session_result(&mut slot, result).await
    }

    pub async fn click(&self, selector: &str) -> Result<(), ChromeToolError> {
        let mut slot = self.session.lock().await;
        let session = Self::active_session(&slot)?;
        let result = session.interaction().click(selector).await;
        Self::clear_stale_session_result(&mut slot, result).await
    }

    pub async fn type_text(&self, selector: &str, text: &str) -> Result<(), ChromeToolError> {
        let mut slot = self.session.lock().await;
        let session = Self::active_session(&slot)?;
        let result = session.interaction().type_text(selector, text).await;
        Self::clear_stale_session_result(&mut slot, result).await
    }

    pub async fn current_url(&self) -> Result<String, ChromeToolError> {
        let mut slot = self.session.lock().await;
        let session = Self::active_session(&slot)?;
        let result = session.interaction().current_url().await;
        Self::clear_stale_session_result(&mut slot, result).await
    }

    pub async fn with_webdriver<T, F>(&self, operation: F) -> Result<T, ChromeToolError>
    where
        F: for<'driver> FnOnce(
                &'driver WebDriver,
            ) -> Pin<
                Box<dyn Future<Output = Result<T, ChromeToolError>> + Send + 'driver>,
            > + Send,
    {
        // The callback runs while the shared session and WebDriver locks are held.
        // Callers should perform only direct WebDriver operations here.
        let mut slot = self.session.lock().await;
        let session = Self::active_session(&slot)?;
        let interaction = session.interaction();
        let webdriver =
            interaction
                .webdriver_mutex()
                .ok_or_else(|| ChromeToolError::InteractionFailed {
                    reason: "direct webdriver access is not supported by this browser session"
                        .to_string(),
                })?;

        let result = {
            let guard = webdriver.lock().await;
            let driver = guard
                .as_ref()
                .ok_or_else(|| ChromeToolError::SessionShutdownFailed {
                    reason: "session already closed".to_string(),
                })?;
            operation(driver).await
        };

        Self::clear_stale_session_result(&mut slot, result).await
    }

    pub async fn get_cookies(&self, domain: Option<&str>) -> Result<Vec<Cookie>, ChromeToolError> {
        let mut slot = self.session.lock().await;
        let session = Self::active_session(&slot)?;
        let cookies =
            Self::clear_stale_session_result(&mut slot, session.interaction().get_cookies().await)
                .await?;

        let Some(domain) = domain.and_then(normalize_cookie_domain) else {
            return Ok(cookies);
        };

        Ok(cookies
            .into_iter()
            .filter(|cookie| cookie_matches_domain(cookie.domain.as_deref(), &domain))
            .collect())
    }

    pub async fn new_tab(&self, url: &str) -> Result<NewTabResult, ChromeToolError> {
        let mut slot = self.session.lock().await;
        let session = Self::active_session(&slot)?;
        let result =
            Self::clear_stale_session_result(&mut slot, session.create_new_tab(url).await).await?;
        if let Some(session) = slot.as_mut() {
            session.update_metadata(PageMetadata {
                final_url: result.url.clone(),
                page_title: result.page_title.clone(),
            });
        }
        Ok(result)
    }

    pub async fn switch_tab(&self, tab_id: &str) -> Result<PageMetadata, ChromeToolError> {
        let mut slot = self.session.lock().await;
        let session = Self::active_session(&slot)?;
        let metadata =
            Self::clear_stale_session_result(&mut slot, session.switch_tab(tab_id).await).await?;
        if let Some(session) = slot.as_mut() {
            session.update_metadata(metadata.clone());
        }
        Ok(metadata)
    }

    pub async fn close_tab(&self, tab_id: &str) -> Result<(), ChromeToolError> {
        let mut slot = self.session.lock().await;
        let session = Self::active_session(&slot)?;
        let metadata =
            Self::clear_stale_session_result(&mut slot, session.close_tab(tab_id).await).await?;
        if let Some(session) = slot.as_mut() {
            session.update_metadata(metadata);
        }
        Ok(())
    }

    pub async fn list_tabs(&self) -> Result<Vec<TabInfo>, ChromeToolError> {
        let mut slot = self.session.lock().await;
        let session = Self::active_session(&slot)?;
        Self::clear_stale_session_result(&mut slot, session.list_tabs().await).await
    }

    async fn open_new_session(&self, url: &str) -> Result<ChromeSession, ChromeToolError> {
        let BackendOpenResult { metadata, session } = self.backend.open(url).await?;
        let session = ChromeSession::new(
            metadata.final_url.clone(),
            metadata.page_title.clone(),
            session,
        );
        // Register the active window so tab APIs work on freshly opened or re-attached sessions.
        if let Ok(handle) = session.interaction().current_window_handle().await {
            session.register_initial_tab(handle).await;
        }
        Ok(session)
    }

    fn active_session(slot: &Option<ChromeSession>) -> Result<ChromeSession, ChromeToolError> {
        slot.clone()
            .ok_or(ChromeToolError::SharedSessionUnavailable)
    }

    async fn clear_stale_session_result<T>(
        slot: &mut Option<ChromeSession>,
        result: Result<T, ChromeToolError>,
    ) -> Result<T, ChromeToolError> {
        match result {
            Err(error) if Self::is_stale_session_error(&error) => {
                Self::shutdown_slot(slot).await;
                Err(ChromeToolError::SharedSessionUnavailable)
            }
            other => other,
        }
    }

    async fn shutdown_slot(slot: &mut Option<ChromeSession>) {
        if let Some(session) = slot.take() {
            let _ = session.interaction().shutdown().await;
        }
    }

    fn is_stale_session_error(error: &ChromeToolError) -> bool {
        let reason = match error {
            ChromeToolError::NavigationFailed { reason, .. }
            | ChromeToolError::PageReadFailed { reason }
            | ChromeToolError::SessionShutdownFailed { reason }
            | ChromeToolError::InteractionFailed { reason }
            | ChromeToolError::TabOperationFailed { reason } => reason,
            ChromeToolError::InvalidArguments { .. }
            | ChromeToolError::MissingRequiredField { .. }
            | ChromeToolError::ActionNotAllowed { .. }
            | ChromeToolError::SharedSessionUnavailable
            | ChromeToolError::DirectoryCreateFailed { .. }
            | ChromeToolError::DriverDownloadFailed { .. }
            | ChromeToolError::ChromeNotInstalled
            | ChromeToolError::DriverNotInstalled { .. }
            | ChromeToolError::InstallUnavailable
            | ChromeToolError::UnsupportedPlatform { .. }
            | ChromeToolError::ChromeVersionDetectFailed { .. }
            | ChromeToolError::DriverArchiveInvalid { .. }
            | ChromeToolError::DriverPatchFailed { .. }
            | ChromeToolError::DriverStartFailed { .. }
            | ChromeToolError::FileReadFailed { .. }
            | ChromeToolError::FileWriteFailed { .. }
            | ChromeToolError::TabNotFound { .. }
            | ChromeToolError::CannotCloseLastTab
            | ChromeToolError::OutputSerialization(_) => return false,
        };

        let reason = reason.to_ascii_lowercase();
        [
            "session already closed",
            "invalid session",
            "session deleted",
            "chrome not reachable",
            "disconnected",
            "target window already closed",
            "no such window",
            "web view not found",
        ]
        .iter()
        .any(|needle| reason.contains(needle))
    }
}

fn opened_page_from_session(session: &ChromeSession) -> OpenedPage {
    OpenedPage {
        final_url: session.current_url.clone(),
        page_title: session.page_title.clone(),
    }
}

fn cookie_matches_domain(cookie_domain: Option<&str>, requested_domain: &str) -> bool {
    let Some(cookie_domain) = cookie_domain.and_then(normalize_cookie_domain) else {
        return false;
    };

    requested_domain == cookie_domain || requested_domain.ends_with(&format!(".{cookie_domain}"))
}

fn normalize_cookie_domain(domain: &str) -> Option<String> {
    let normalized = domain.trim().trim_start_matches('.').to_ascii_lowercase();
    if normalized.is_empty() {
        None
    } else {
        Some(normalized)
    }
}

impl Drop for ChromeManager {
    fn drop(&mut self) {
        let session = self.session.get_mut().take();
        let shared_host = self
            .managed_support
            .as_ref()
            .and_then(|support| support.shared_host.clone());
        if session.is_none() && shared_host.is_none() {
            return;
        }

        let interaction = session.map(|session| session.interaction());
        let _ = std::thread::spawn(move || {
            if let Ok(runtime) = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                runtime.block_on(async move {
                    if let Some(interaction) = interaction {
                        let _ = interaction.shutdown().await;
                    }
                    if let Some(shared_host) = shared_host {
                        let _ = shared_host.shutdown().await;
                    }
                });
            }
        })
        .join();
    }
}

#[derive(Debug)]
pub struct SystemChromeHost {
    driver_process: Mutex<Option<Arc<ChromeDriverProcess>>>,
}

impl Default for SystemChromeHost {
    fn default() -> Self {
        Self {
            driver_process: Mutex::new(None),
        }
    }
}

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
        let process = self.ensure_driver_process(driver_binary).await?;
        let server_url = process.server_url();

        let capabilities =
            build_chrome_capabilities(browser_binary, browser_version, session_mode)?;

        let driver = match WebDriver::new(&server_url, capabilities).await {
            Ok(driver) => driver,
            Err(err) => {
                return Err(ChromeToolError::DriverStartFailed {
                    reason: err.to_string(),
                });
            }
        };

        if let Err(err) = driver.goto(url).await {
            let _ = driver.quit().await;
            return Err(ChromeToolError::NavigationFailed {
                url: url.to_string(),
                reason: err.to_string(),
            });
        }
        let final_url = driver
            .current_url()
            .await
            .map(|u| u.to_string())
            .map_err(|e| ChromeToolError::PageReadFailed {
                reason: e.to_string(),
            })?;
        let page_title = driver
            .title()
            .await
            .map_err(|e| ChromeToolError::PageReadFailed {
                reason: e.to_string(),
            })?;

        let session: Arc<dyn BrowserSession> = Arc::new(Mutex::new(Some(driver)));
        Ok(BackendOpenResult {
            metadata: PageMetadata {
                final_url,
                page_title,
            },
            session,
        })
    }
}

impl SystemChromeHost {
    async fn shutdown(&self) -> Result<(), ChromeToolError> {
        let process = { self.driver_process.lock().await.take() };
        if let Some(process) = process {
            process.shutdown().await?;
        }
        Ok(())
    }

    #[cfg(test)]
    async fn take_reusable_driver_process(
        &self,
        driver_binary: &Path,
    ) -> Result<Option<Arc<ChromeDriverProcess>>, ChromeToolError> {
        let mut guard = self.driver_process.lock().await;
        let Some(process) = guard.as_ref().cloned() else {
            return Ok(None);
        };

        if process.matches_driver_binary(driver_binary) && process.is_alive().await {
            return Ok(Some(process));
        }

        guard.take();
        process.shutdown().await?;
        Ok(None)
    }

    async fn ensure_driver_process(
        &self,
        driver_binary: &Path,
    ) -> Result<Arc<ChromeDriverProcess>, ChromeToolError> {
        let mut guard = self.driver_process.lock().await;
        if let Some(process) = guard.as_ref().cloned() {
            if process.matches_driver_binary(driver_binary) && process.is_alive().await {
                return Ok(process);
            }
            guard.take();
            process.shutdown().await?;
        }

        let mut command = background_command(driver_binary);
        let mut child = command
            .arg(format!("--port={SHARED_DRIVER_PORT}"))
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| ChromeToolError::DriverStartFailed {
                reason: e.to_string(),
            })?;

        if let Err(err) = wait_for_driver_ready(&mut child, SHARED_DRIVER_PORT).await {
            let _ = shutdown_child_process(&mut child).await;
            return Err(err);
        }

        let process = Arc::new(ChromeDriverProcess::new(
            child,
            SHARED_DRIVER_PORT,
            driver_binary.to_path_buf(),
        ));
        *guard = Some(Arc::clone(&process));
        Ok(process)
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
        let mut command = background_command("powershell");
        command.arg("-NoProfile").arg("-Command").arg(format!(
            "(Get-Item '{}').VersionInfo.ProductVersion",
            escaped_path
        ));
        command
    } else {
        let mut command = background_command(browser_binary);
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

fn background_command(program: impl AsRef<std::ffi::OsStr>) -> Command {
    let mut command = Command::new(program);
    configure_background_command(&mut command);
    command
}

fn configure_background_command(command: &mut Command) {
    #[cfg(windows)]
    {
        command
            .as_std_mut()
            .creation_flags(background_command_creation_flags_for_os("windows"));
    }

    #[cfg(not(windows))]
    {
        let _ = command;
    }
}

#[cfg(any(test, windows))]
fn background_command_creation_flags_for_os(os: &str) -> u32 {
    if os == "windows" {
        WINDOWS_CREATE_NO_WINDOW
    } else {
        0
    }
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
    #[cfg(unix)]
    use std::process::Stdio;
    use std::sync::Arc;
    use std::sync::Mutex as StdMutex;
    use std::sync::atomic::{AtomicUsize, Ordering as AtomicOrdering};
    use std::time::Duration;

    use serde_json::json;
    use tempfile::tempdir;
    use thirtyfour::common::cookie::Cookie;
    use thirtyfour::prelude::{DesiredCapabilities, WebDriver};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;
    #[cfg(unix)]
    use tokio::process::Command;
    use tokio::sync::Mutex as TokioMutex;

    use crate::chrome::error::ChromeToolError;
    use crate::chrome::installer::{ChromeInstaller, ChromePaths, DriverDownloader};
    use crate::chrome::models::PageMetadata;
    use crate::chrome::session::{BrowserSession, ChromeDriverProcess};

    use super::{
        BackendOpenResult, BrowserBackend, ChromeHost, ChromeManager, ManagedChromeSupport,
        SessionMode, SystemChromeHost, WINDOWS_CREATE_NO_WINDOW,
        background_command_creation_flags_for_os, build_chrome_capabilities,
        interactive_user_agent_for_os, parse_browser_version_output,
    };

    #[derive(Debug, Clone)]
    struct FakePage {
        final_url: String,
        page_title: String,
        text: String,
        shutdown_label: String,
        extra_tabs: Vec<FakeTab>,
        cookies: Vec<Cookie>,
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
            _links: Vec<()>,
            text: impl Into<String>,
        ) -> Self {
            let requested_url = requested_url.into();
            self.pages.insert(
                requested_url.clone(),
                FakePage {
                    final_url: final_url.into(),
                    page_title: page_title.into(),
                    text: text.into(),
                    shutdown_label: requested_url,
                    extra_tabs: Vec::new(),
                    cookies: Vec::new(),
                },
            );
            self
        }

        fn with_page_with_extra_tabs(
            mut self,
            requested_url: impl Into<String>,
            final_url: impl Into<String>,
            page_title: impl Into<String>,
            _links: Vec<()>,
            text: impl Into<String>,
            extra_tabs: Vec<FakeTab>,
        ) -> Self {
            let requested_url = requested_url.into();
            self.pages.insert(
                requested_url.clone(),
                FakePage {
                    final_url: final_url.into(),
                    page_title: page_title.into(),
                    text: text.into(),
                    shutdown_label: requested_url,
                    extra_tabs,
                    cookies: Vec::new(),
                },
            );
            self
        }
    }

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct FakeTab {
        handle: String,
        url: String,
        title: String,
    }

    #[derive(Debug)]
    struct FakeBrowserTabState {
        tabs: Vec<FakeTab>,
        active_handle: Option<String>,
    }

    #[derive(Debug)]
    struct FakeBrowserSession {
        text: String,
        shutdown_label: String,
        shutdowns: Arc<StdMutex<Vec<String>>>,
        cookies: Vec<Cookie>,
        tabs: StdMutex<FakeBrowserTabState>,
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
            let tabs = self.tabs.lock().unwrap();
            let active_handle = tabs.active_handle.as_deref().ok_or_else(|| {
                ChromeToolError::TabOperationFailed {
                    reason: "no active tab".to_string(),
                }
            })?;
            tabs.tabs
                .iter()
                .find(|tab| tab.handle == active_handle)
                .map(|tab| tab.url.clone())
                .ok_or_else(|| ChromeToolError::TabOperationFailed {
                    reason: "active tab not found".to_string(),
                })
        }

        async fn get_cookies(&self) -> Result<Vec<Cookie>, ChromeToolError> {
            Ok(self.cookies.clone())
        }

        async fn navigate(&self, url: &str) -> Result<PageMetadata, ChromeToolError> {
            let mut tabs = self.tabs.lock().unwrap();
            let active_handle =
                tabs.active_handle
                    .clone()
                    .ok_or_else(|| ChromeToolError::TabOperationFailed {
                        reason: "no active tab".to_string(),
                    })?;
            let tab = tabs
                .tabs
                .iter_mut()
                .find(|tab| tab.handle == active_handle)
                .ok_or_else(|| ChromeToolError::TabOperationFailed {
                    reason: "active tab not found".to_string(),
                })?;
            tab.url = url.to_string();
            tab.title = format!("Navigated to {url}");
            Ok(PageMetadata {
                final_url: tab.url.clone(),
                page_title: tab.title.clone(),
            })
        }

        async fn create_new_tab(
            &self,
            url: &str,
        ) -> Result<(String, PageMetadata), ChromeToolError> {
            let mut tabs = self.tabs.lock().unwrap();
            let handle = format!("handle-{}", tabs.tabs.len() + 1);
            let metadata = PageMetadata {
                final_url: url.to_string(),
                page_title: format!("Tab {url}"),
            };
            tabs.tabs.push(FakeTab {
                handle: handle.clone(),
                url: metadata.final_url.clone(),
                title: metadata.page_title.clone(),
            });
            tabs.active_handle = Some(handle.clone());
            Ok((handle, metadata))
        }

        async fn switch_to_window(
            &self,
            window_handle: &str,
        ) -> Result<PageMetadata, ChromeToolError> {
            let mut tabs = self.tabs.lock().unwrap();
            let tab = tabs
                .tabs
                .iter()
                .find(|tab| tab.handle == window_handle)
                .cloned()
                .ok_or_else(|| ChromeToolError::TabNotFound {
                    tab_id: window_handle.to_string(),
                })?;
            tabs.active_handle = Some(window_handle.to_string());
            Ok(PageMetadata {
                final_url: tab.url,
                page_title: tab.title,
            })
        }

        async fn close_current_window(&self) -> Result<(), ChromeToolError> {
            let mut tabs = self.tabs.lock().unwrap();
            let Some(active_handle) = tabs.active_handle.clone() else {
                return Err(ChromeToolError::TabOperationFailed {
                    reason: "no active tab".to_string(),
                });
            };
            tabs.tabs.retain(|tab| tab.handle != active_handle);
            tabs.active_handle = tabs.tabs.first().map(|tab| tab.handle.clone());
            Ok(())
        }

        async fn list_windows(&self) -> Result<Vec<(String, String, String)>, ChromeToolError> {
            let tabs = self.tabs.lock().unwrap();
            Ok(tabs
                .tabs
                .iter()
                .map(|tab| (tab.handle.clone(), tab.url.clone(), tab.title.clone()))
                .collect())
        }

        async fn current_window_handle(&self) -> Result<String, ChromeToolError> {
            self.tabs
                .lock()
                .unwrap()
                .active_handle
                .clone()
                .ok_or_else(|| ChromeToolError::TabOperationFailed {
                    reason: "no tabs".to_string(),
                })
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
                text: page.text.clone(),
                shutdown_label: page.shutdown_label.clone(),
                shutdowns: Arc::clone(&self.shutdowns),
                cookies: page.cookies.clone(),
                tabs: StdMutex::new(FakeBrowserTabState {
                    tabs: {
                        let mut tabs = vec![FakeTab {
                            handle: "handle-1".to_string(),
                            url: page.final_url.clone(),
                            title: page.page_title.clone(),
                        }];
                        tabs.extend(page.extra_tabs.clone());
                        tabs
                    },
                    active_handle: Some("handle-1".to_string()),
                }),
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

    #[derive(Debug, Default)]
    struct FlakyNavigateBackend {
        open_urls: Arc<StdMutex<Vec<String>>>,
        shutdowns: Arc<StdMutex<Vec<String>>>,
    }

    impl FlakyNavigateBackend {
        fn open_urls(&self) -> Vec<String> {
            self.open_urls.lock().unwrap().clone()
        }

        fn shutdowns(&self) -> Vec<String> {
            self.shutdowns.lock().unwrap().clone()
        }
    }

    #[derive(Debug)]
    struct FlakyNavigateSession {
        current_url: StdMutex<String>,
        current_title: StdMutex<String>,
        fail_navigation: bool,
        shutdown_label: String,
        shutdowns: Arc<StdMutex<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl BrowserSession for FlakyNavigateSession {
        async fn extract_text(&self, _selector: Option<&str>) -> Result<String, ChromeToolError> {
            Ok(String::new())
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
            Ok(self.current_url.lock().unwrap().clone())
        }

        async fn get_cookies(&self) -> Result<Vec<Cookie>, ChromeToolError> {
            Ok(Vec::new())
        }

        async fn navigate(&self, url: &str) -> Result<PageMetadata, ChromeToolError> {
            if self.fail_navigation {
                return Err(ChromeToolError::NavigationFailed {
                    url: url.to_string(),
                    reason: "chrome not reachable: target window already closed".to_string(),
                });
            }

            *self.current_url.lock().unwrap() = url.to_string();
            *self.current_title.lock().unwrap() = format!("Navigated to {url}");
            Ok(PageMetadata {
                final_url: self.current_url.lock().unwrap().clone(),
                page_title: self.current_title.lock().unwrap().clone(),
            })
        }

        async fn create_new_tab(
            &self,
            _url: &str,
        ) -> Result<(String, PageMetadata), ChromeToolError> {
            Err(ChromeToolError::TabOperationFailed {
                reason: "not implemented in flaky test session".to_string(),
            })
        }

        async fn switch_to_window(
            &self,
            _window_handle: &str,
        ) -> Result<PageMetadata, ChromeToolError> {
            Err(ChromeToolError::TabOperationFailed {
                reason: "not implemented in flaky test session".to_string(),
            })
        }

        async fn close_current_window(&self) -> Result<(), ChromeToolError> {
            Err(ChromeToolError::TabOperationFailed {
                reason: "not implemented in flaky test session".to_string(),
            })
        }

        async fn list_windows(&self) -> Result<Vec<(String, String, String)>, ChromeToolError> {
            Ok(vec![(
                "handle-1".to_string(),
                self.current_url.lock().unwrap().clone(),
                self.current_title.lock().unwrap().clone(),
            )])
        }

        async fn current_window_handle(&self) -> Result<String, ChromeToolError> {
            Ok("handle-1".to_string())
        }
    }

    #[async_trait::async_trait]
    impl BrowserBackend for FlakyNavigateBackend {
        async fn open(&self, url: &str) -> Result<BackendOpenResult, ChromeToolError> {
            let mut open_urls = self.open_urls.lock().unwrap();
            open_urls.push(url.to_string());
            let open_count = open_urls.len();
            drop(open_urls);

            let session: Arc<dyn BrowserSession> = Arc::new(FlakyNavigateSession {
                current_url: StdMutex::new(url.to_string()),
                current_title: StdMutex::new(format!("Opened {url}")),
                fail_navigation: open_count == 1,
                shutdown_label: format!("open-{open_count}"),
                shutdowns: Arc::clone(&self.shutdowns),
            });

            Ok(BackendOpenResult {
                metadata: PageMetadata {
                    final_url: url.to_string(),
                    page_title: format!("Opened {url}"),
                },
                session,
            })
        }
    }

    #[derive(Debug, Default)]
    struct SlowOpenBackend {
        open_count: AtomicUsize,
    }

    impl SlowOpenBackend {
        fn open_count(&self) -> usize {
            self.open_count.load(AtomicOrdering::SeqCst)
        }
    }

    #[async_trait::async_trait]
    impl BrowserBackend for SlowOpenBackend {
        async fn open(&self, url: &str) -> Result<BackendOpenResult, ChromeToolError> {
            self.open_count.fetch_add(1, AtomicOrdering::SeqCst);
            tokio::time::sleep(Duration::from_millis(50)).await;

            let session: Arc<dyn BrowserSession> = Arc::new(FlakyNavigateSession {
                current_url: StdMutex::new(url.to_string()),
                current_title: StdMutex::new(format!("Opened {url}")),
                fail_navigation: false,
                shutdown_label: url.to_string(),
                shutdowns: Arc::new(StdMutex::new(Vec::new())),
            });

            Ok(BackendOpenResult {
                metadata: PageMetadata {
                    final_url: url.to_string(),
                    page_title: format!("Opened {url}"),
                },
                session,
            })
        }
    }

    #[derive(Debug, Default)]
    struct StaleReadBackend {
        shutdowns: Arc<StdMutex<Vec<String>>>,
    }

    #[derive(Debug)]
    struct StaleReadSession {
        shutdowns: Arc<StdMutex<Vec<String>>>,
    }

    #[async_trait::async_trait]
    impl BrowserSession for StaleReadSession {
        async fn extract_text(&self, _selector: Option<&str>) -> Result<String, ChromeToolError> {
            Err(ChromeToolError::PageReadFailed {
                reason: "invalid session id".to_string(),
            })
        }

        async fn shutdown(&self) -> Result<(), ChromeToolError> {
            self.shutdowns
                .lock()
                .unwrap()
                .push("stale-read".to_string());
            Ok(())
        }

        async fn click(&self, _selector: &str) -> Result<(), ChromeToolError> {
            Ok(())
        }

        async fn type_text(&self, _selector: &str, _text: &str) -> Result<(), ChromeToolError> {
            Ok(())
        }

        async fn current_url(&self) -> Result<String, ChromeToolError> {
            Ok("https://example.com".to_string())
        }

        async fn get_cookies(&self) -> Result<Vec<Cookie>, ChromeToolError> {
            Ok(Vec::new())
        }

        async fn navigate(&self, url: &str) -> Result<PageMetadata, ChromeToolError> {
            Ok(PageMetadata {
                final_url: url.to_string(),
                page_title: format!("Navigated to {url}"),
            })
        }

        async fn create_new_tab(
            &self,
            _url: &str,
        ) -> Result<(String, PageMetadata), ChromeToolError> {
            Err(ChromeToolError::TabOperationFailed {
                reason: "not implemented in stale read test session".to_string(),
            })
        }

        async fn switch_to_window(
            &self,
            _window_handle: &str,
        ) -> Result<PageMetadata, ChromeToolError> {
            Err(ChromeToolError::TabOperationFailed {
                reason: "not implemented in stale read test session".to_string(),
            })
        }

        async fn close_current_window(&self) -> Result<(), ChromeToolError> {
            Err(ChromeToolError::TabOperationFailed {
                reason: "not implemented in stale read test session".to_string(),
            })
        }

        async fn list_windows(&self) -> Result<Vec<(String, String, String)>, ChromeToolError> {
            Ok(vec![(
                "handle-1".to_string(),
                "https://example.com".to_string(),
                "Example".to_string(),
            )])
        }

        async fn current_window_handle(&self) -> Result<String, ChromeToolError> {
            Ok("handle-1".to_string())
        }
    }

    #[async_trait::async_trait]
    impl BrowserBackend for StaleReadBackend {
        async fn open(&self, url: &str) -> Result<BackendOpenResult, ChromeToolError> {
            let session: Arc<dyn BrowserSession> = Arc::new(StaleReadSession {
                shutdowns: Arc::clone(&self.shutdowns),
            });

            Ok(BackendOpenResult {
                metadata: PageMetadata {
                    final_url: url.to_string(),
                    page_title: "Example".to_string(),
                },
                session,
            })
        }
    }

    #[derive(Debug)]
    struct WebDriverBackend {
        driver: StdMutex<Option<WebDriver>>,
    }

    #[async_trait::async_trait]
    impl BrowserBackend for WebDriverBackend {
        async fn open(&self, url: &str) -> Result<BackendOpenResult, ChromeToolError> {
            let driver = self
                .driver
                .lock()
                .unwrap()
                .take()
                .expect("test webdriver should be opened once");
            let session: Arc<dyn BrowserSession> = Arc::new(TokioMutex::new(Some(driver)));

            Ok(BackendOpenResult {
                metadata: PageMetadata {
                    final_url: url.to_string(),
                    page_title: "Initial".to_string(),
                },
                session,
            })
        }
    }

    async fn read_http_request(stream: &mut tokio::net::TcpStream) -> std::io::Result<String> {
        let mut buffer = Vec::new();
        let mut chunk = [0_u8; 1024];
        let mut header_end = None;
        let mut content_length = 0_usize;

        loop {
            let read = stream.read(&mut chunk).await?;
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);

            if header_end.is_none()
                && let Some(index) = buffer.windows(4).position(|window| window == b"\r\n\r\n")
            {
                header_end = Some(index + 4);
                let headers = String::from_utf8_lossy(&buffer[..index + 4]);
                for line in headers.lines() {
                    if let Some(value) = line.strip_prefix("Content-Length:") {
                        content_length = value.trim().parse().unwrap_or(0);
                    }
                }
            }

            if let Some(end) = header_end
                && buffer.len() >= end + content_length
            {
                return Ok(String::from_utf8_lossy(&buffer[..end])
                    .lines()
                    .next()
                    .unwrap_or_default()
                    .to_string());
            }
        }

        Ok(String::from_utf8_lossy(&buffer)
            .lines()
            .next()
            .unwrap_or_default()
            .to_string())
    }

    async fn write_json_response(
        stream: &mut tokio::net::TcpStream,
        status: &str,
        body: serde_json::Value,
    ) -> std::io::Result<()> {
        let body = body.to_string();
        let response = format!(
            "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).await?;
        stream.shutdown().await
    }

    fn sample_backend() -> Arc<FakeBrowserBackend> {
        Arc::new(
            FakeBrowserBackend::default()
                .with_page(
                    "https://example.com",
                    "https://example.com",
                    "Example",
                    Vec::new(),
                    "Example text",
                )
                .with_page(
                    "https://example.org",
                    "https://example.org/home",
                    "Example Org",
                    Vec::new(),
                    "Org text",
                ),
        )
    }

    #[tokio::test]
    async fn manager_with_webdriver_runs_operation_inside_locked_session() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move {
            for expected_request in [
                "POST /session HTTP/1.1",
                "POST /session/test-session/timeouts HTTP/1.1",
                "GET /session/test-session/window HTTP/1.1",
                "GET /session/test-session/url HTTP/1.1",
                "DELETE /session/test-session HTTP/1.1",
            ] {
                let (mut stream, _) = listener.accept().await.unwrap();
                let request_line = read_http_request(&mut stream).await.unwrap();
                assert_eq!(request_line, expected_request);

                let response = if expected_request.starts_with("POST /session ") {
                    json!({
                        "value": {
                            "sessionId": "test-session",
                            "capabilities": {
                                "browserName": "chrome"
                            }
                        }
                    })
                } else if expected_request.ends_with("/window HTTP/1.1") {
                    json!({ "value": "window-1" })
                } else if expected_request.ends_with("/url HTTP/1.1") {
                    json!({ "value": "https://example.com/locked" })
                } else {
                    json!({ "value": null })
                };

                write_json_response(&mut stream, "200 OK", response)
                    .await
                    .unwrap();
            }
        });

        let driver = WebDriver::new(
            format!("http://{server_addr}"),
            DesiredCapabilities::chrome(),
        )
        .await
        .unwrap();
        let manager = ChromeManager::new_with_components(
            Arc::new(WebDriverBackend {
                driver: StdMutex::new(Some(driver)),
            }),
            None,
            ChromePaths::from_home(tempdir().unwrap().path()),
        );

        manager.navigate("https://example.com").await.unwrap();
        let current_url = manager
            .with_webdriver(|driver| {
                Box::pin(async move {
                    driver
                        .current_url()
                        .await
                        .map(|url| url.to_string())
                        .map_err(|error| ChromeToolError::PageReadFailed {
                            reason: error.to_string(),
                        })
                })
            })
            .await
            .unwrap();

        assert_eq!(current_url, "https://example.com/locked");
        manager.close().await.unwrap();
        server.await.unwrap();
    }

    #[tokio::test]
    async fn manager_serializes_concurrent_initial_navigate_to_one_backend_open() {
        let backend = Arc::new(SlowOpenBackend::default());
        let manager = ChromeManager::new_with_components(
            backend.clone(),
            None,
            ChromePaths::from_home(tempdir().unwrap().path()),
        );

        let (first, second) = tokio::join!(
            manager.navigate("https://example.com/one"),
            manager.navigate("https://example.com/two")
        );

        first.expect("first navigate should succeed");
        second.expect("second navigate should succeed");
        assert_eq!(backend.open_count(), 1);
    }

    #[tokio::test]
    async fn manager_clears_stale_session_after_read_failure() {
        let backend = Arc::new(StaleReadBackend::default());
        let manager = ChromeManager::new_with_components(
            backend.clone(),
            None,
            ChromePaths::from_home(tempdir().unwrap().path()),
        );

        manager
            .navigate("https://example.com")
            .await
            .expect("initial navigate should open stale test session");

        let err = manager.extract_text(None).await.unwrap_err();
        assert!(matches!(err, ChromeToolError::SharedSessionUnavailable));
        assert_eq!(
            backend.shutdowns.lock().unwrap().as_slice(),
            &["stale-read".to_string()]
        );
    }

    #[derive(Debug, Default)]
    struct PanicDownloader;

    #[async_trait::async_trait]
    impl DriverDownloader for PanicDownloader {
        async fn fetch(&self, url: &str) -> Result<Vec<u8>, ChromeToolError> {
            panic!("unexpected download request in test: {url}");
        }
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
            manager.navigate("https://example.com/drop").await.unwrap();
        });
        drop(runtime);
        drop(manager);

        assert_eq!(
            shutdowns.lock().unwrap().as_slice(),
            &["https://example.com/drop".to_string()]
        );
    }

    #[cfg(unix)]
    #[test]
    fn manager_drop_shuts_down_shared_driver_even_without_live_sessions() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();
        let host = Arc::new(SystemChromeHost::default());
        let shared_process = runtime.block_on(async {
            let child = Command::new("sleep")
                .arg("30")
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .unwrap();
            let process = Arc::new(ChromeDriverProcess::new(
                child,
                9515,
                Path::new("/tmp/chromedriver-v1").to_path_buf(),
            ));
            *host.driver_process.lock().await = Some(Arc::clone(&process));
            process
        });

        let home = tempdir().unwrap();
        let paths = ChromePaths::from_home(home.path());
        let installer = Arc::new(ChromeInstaller::new(
            paths.clone(),
            Arc::new(PanicDownloader),
        ));
        let host_trait: Arc<dyn ChromeHost> = host.clone();
        let manager = ChromeManager::new_with_components(
            sample_backend(),
            Some(ManagedChromeSupport {
                host: host_trait,
                installer,
                shared_host: Some(host.clone()),
            }),
            paths,
        );

        drop(manager);

        assert!(!runtime.block_on(shared_process.is_alive()));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn system_chrome_host_does_not_reuse_cached_process_for_different_driver_binary() {
        let host = SystemChromeHost::default();
        let child = Command::new("sleep")
            .arg("30")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let cached_process = Arc::new(ChromeDriverProcess::new(
            child,
            9515,
            Path::new("/tmp/chromedriver-v1").to_path_buf(),
        ));
        *host.driver_process.lock().await = Some(Arc::clone(&cached_process));

        let reused = host
            .take_reusable_driver_process(Path::new("/tmp/chromedriver-v2"))
            .await
            .unwrap();

        assert!(reused.is_none());
        assert!(!cached_process.is_alive().await);
        assert!(host.driver_process.lock().await.is_none());
    }

    #[tokio::test]
    async fn manager_navigate_creates_session_and_returns_metadata() {
        let manager = ChromeManager::new_for_test(sample_backend());

        let opened = manager.navigate("https://example.com").await.unwrap();

        assert_eq!(opened.final_url, "https://example.com");
        assert_eq!(opened.page_title, "Example");
    }

    #[tokio::test]
    async fn manager_reopens_single_session_when_navigation_fails_after_browser_closes() {
        let backend = Arc::new(FlakyNavigateBackend::default());
        let temp_dir = tempdir().unwrap();
        let manager = ChromeManager::new_with_components(
            backend.clone(),
            None,
            ChromePaths::from_home(temp_dir.path()),
        );

        manager
            .navigate("https://example.com")
            .await
            .expect("initial open should succeed");

        let reopened = manager
            .navigate("https://example.org")
            .await
            .expect("navigate should recover by reopening a fresh session");

        assert_eq!(reopened.final_url, "https://example.org");
        assert_eq!(reopened.page_title, "Opened https://example.org");
        assert_eq!(
            backend.open_urls(),
            vec![
                "https://example.com".to_string(),
                "https://example.org".to_string()
            ]
        );
        assert_eq!(backend.shutdowns(), vec!["open-1".to_string()]);
    }

    #[tokio::test]
    async fn manager_reuses_single_session_for_subsequent_navigate() {
        let manager = ChromeManager::new_for_test(sample_backend());

        manager.navigate("https://example.com").await.unwrap();
        let opened = manager.navigate("https://example.org").await.unwrap();

        assert_eq!(opened.final_url, "https://example.org");
        assert_eq!(opened.page_title, "Navigated to https://example.org");
        assert_eq!(manager.current_url().await.unwrap(), "https://example.org");
    }

    #[tokio::test]
    async fn manager_close_removes_session_and_shuts_it_down() {
        let shutdowns = Arc::new(StdMutex::new(Vec::new()));
        let manager = ChromeManager::new_for_test(Arc::new(
            FakeBrowserBackend::default()
                .with_shutdowns(Arc::clone(&shutdowns))
                .with_page(
                    "https://example.com",
                    "https://example.com",
                    "Example",
                    Vec::new(),
                    "Example text",
                ),
        ));

        manager.navigate("https://example.com").await.unwrap();
        manager.close().await.unwrap();

        let err = manager.extract_text(None).await.unwrap_err();
        assert!(matches!(err, ChromeToolError::SharedSessionUnavailable));
        assert_eq!(
            shutdowns.lock().unwrap().as_slice(),
            &["https://example.com".to_string()]
        );
    }

    #[tokio::test]
    async fn manager_uses_active_session_for_read_operations() {
        let manager = ChromeManager::new_for_test(sample_backend());
        manager.navigate("https://example.com").await.unwrap();

        let text = manager.extract_text(Some("#hero")).await.unwrap();
        assert_eq!(text, "Example text [#hero]");
    }

    #[tokio::test]
    async fn manager_close_active_tab_updates_session_metadata() {
        let manager = ChromeManager::new_for_test(sample_backend());
        manager.navigate("https://example.com").await.unwrap();

        let new_tab = manager.new_tab("https://example.org").await.unwrap();
        manager.switch_tab(&new_tab.tab_id).await.unwrap();

        tokio::time::timeout(
            Duration::from_millis(200),
            manager.close_tab(&new_tab.tab_id),
        )
        .await
        .expect("close_tab should not deadlock")
        .unwrap();

        assert_eq!(manager.current_url().await.unwrap(), "https://example.com");

        let tabs = manager.list_tabs().await.unwrap();
        assert_eq!(tabs.len(), 1);
        assert!(tabs[0].active);
        assert_eq!(tabs[0].url, "https://example.com");
    }

    #[tokio::test]
    async fn manager_list_tabs_returns_ids_that_can_be_switched_to() {
        let manager = ChromeManager::new_for_test(Arc::new(
            FakeBrowserBackend::default().with_page_with_extra_tabs(
                "https://example.com",
                "https://example.com",
                "Example",
                vec![],
                "Example text",
                vec![FakeTab {
                    handle: "popup-1".to_string(),
                    url: "https://example.org/popup".to_string(),
                    title: "Popup".to_string(),
                }],
            ),
        ));

        manager.navigate("https://example.com").await.unwrap();

        let tabs = manager.list_tabs().await.unwrap();
        assert_eq!(tabs.len(), 2);
        let popup = tabs
            .iter()
            .find(|tab| tab.url == "https://example.org/popup")
            .expect("popup tab should be listed");

        let metadata = manager.switch_tab(&popup.tab_id).await.unwrap();
        assert_eq!(metadata.final_url, "https://example.org/popup");
        assert_eq!(metadata.page_title, "Popup");
    }

    #[tokio::test]
    async fn manager_api_requires_active_session_for_all_session_ops() {
        let manager = ChromeManager::new_for_test(sample_backend());

        let err = manager.extract_text(None).await.unwrap_err();
        assert!(matches!(err, ChromeToolError::SharedSessionUnavailable));

        let err = manager.current_url().await.unwrap_err();
        assert!(matches!(err, ChromeToolError::SharedSessionUnavailable));

        let err = manager.get_cookies(None).await.unwrap_err();
        assert!(matches!(err, ChromeToolError::SharedSessionUnavailable));

        let err = manager.list_tabs().await.unwrap_err();
        assert!(matches!(err, ChromeToolError::SharedSessionUnavailable));
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
    fn background_command_creation_flags_hide_console_on_windows() {
        assert_eq!(
            background_command_creation_flags_for_os("windows"),
            WINDOWS_CREATE_NO_WINDOW
        );
        assert_eq!(background_command_creation_flags_for_os("macos"), 0);
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
        assert!(caps_json.get("goog:loggingPrefs").is_none());
        assert!(caps_json["goog:chromeOptions"]["perfLoggingPrefs"].is_null());
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
        assert!(caps_json.get("goog:loggingPrefs").is_none());
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
        assert!(caps_json.get("goog:loggingPrefs").is_none());
        assert!(caps_json["goog:chromeOptions"]["perfLoggingPrefs"].is_null());
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
