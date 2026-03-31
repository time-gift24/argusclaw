use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::thread;
use std::time::{Duration, SystemTime};

use async_trait::async_trait;
use reqwest::Client;
use tokio::sync::Mutex;
use zip::ZipArchive;

use super::error::ChromeToolError;
use super::patcher::patch_cdc_tokens;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChromePaths {
    pub root: PathBuf,
    pub driver: PathBuf,
    pub patched: PathBuf,
    pub tmp: PathBuf,
}

impl ChromePaths {
    #[must_use]
    pub fn from_home(home: &Path) -> Self {
        let root = home.join(".arguswing").join("chrome");
        let driver = root.join("driver");
        let patched = root.join("patched");
        let tmp = root.join("tmp");

        Self {
            root,
            driver,
            patched,
            tmp,
        }
    }

    pub fn ensure_directories(&self) -> Result<(), ChromeToolError> {
        create_directory(&self.root)?;
        create_directory(&self.driver)?;
        create_directory(&self.patched)?;
        create_directory(&self.tmp)?;
        Ok(())
    }
}

fn create_directory(path: &Path) -> Result<(), ChromeToolError> {
    std::fs::create_dir_all(path).map_err(|e| ChromeToolError::DirectoryCreateFailed {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstalledDriver {
    pub original_driver: PathBuf,
    pub patched_driver: PathBuf,
    pub driver_version: String,
    pub cache_hit: bool,
}

#[async_trait]
pub trait DriverDownloader: Send + Sync {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, ChromeToolError>;
}

pub struct ChromeInstaller {
    paths: ChromePaths,
    downloader: Arc<dyn DriverDownloader>,
    install_lock: Mutex<()>,
}

impl ChromeInstaller {
    #[must_use]
    pub fn new(paths: ChromePaths, downloader: Arc<dyn DriverDownloader>) -> Self {
        Self {
            paths,
            downloader,
            install_lock: Mutex::new(()),
        }
    }

    pub async fn ensure_driver(
        &self,
        chrome_version: &str,
    ) -> Result<InstalledDriver, ChromeToolError> {
        let _guard = self.install_lock.lock().await;
        self.paths.ensure_directories()?;
        let platform = ChromePlatform::detect()?;
        let _file_lock = InstallLockGuard::acquire(&self.paths.root.join(".install.lock"))?;
        if let Some(cached) = self.find_cached_install(chrome_version, &platform)? {
            return Ok(cached);
        }
        let version = self.resolve_driver_version(chrome_version).await?;
        let original_driver = self
            .paths
            .driver
            .join(driver_filename(version.as_str(), &platform));
        let patched_driver = self
            .paths
            .patched
            .join(driver_filename(version.as_str(), &platform));

        if !original_driver.is_file() {
            let archive_url = driver_archive_url(version.as_str(), &platform);
            let archive_bytes = self.downloader.fetch(&archive_url).await?;
            extract_driver_binary(&archive_bytes, &self.paths.tmp, &platform, &original_driver)?;
        }
        if !patched_driver.is_file() {
            let original_bytes =
                std::fs::read(&original_driver).map_err(|e| ChromeToolError::FileReadFailed {
                    path: original_driver.clone(),
                    reason: e.to_string(),
                })?;
            let patched_bytes = patch_cdc_tokens(original_bytes, b'X').map_err(|e| {
                ChromeToolError::DriverPatchFailed {
                    path: original_driver.clone(),
                    reason: e.to_string(),
                }
            })?;
            atomic_write_file(&self.paths.tmp, &patched_driver, &patched_bytes)?;
            ensure_executable(&patched_driver)?;
            repair_macos_code_signature(&patched_driver)?;
        }

        Ok(InstalledDriver {
            original_driver,
            patched_driver,
            driver_version: version,
            cache_hit: false,
        })
    }

    pub fn find_installed_driver(
        &self,
        chrome_version: &str,
    ) -> Result<Option<InstalledDriver>, ChromeToolError> {
        let platform = ChromePlatform::detect()?;
        if !self.paths.driver.is_dir() || !self.paths.patched.is_dir() {
            return Ok(None);
        }

        self.find_cached_install(chrome_version, &platform)
    }

    async fn resolve_driver_version(
        &self,
        chrome_version: &str,
    ) -> Result<String, ChromeToolError> {
        match driver_version_source(chrome_version)? {
            DriverVersionSource::ExactVersion => Ok(chrome_version.to_string()),
            DriverVersionSource::MilestoneJson => {
                let url = milestone_versions_url();
                let bytes = self.downloader.fetch(url).await?;
                parse_milestone_version(&bytes, chrome_major_version(chrome_version)?.as_str())
            }
            DriverVersionSource::LegacyLatestRelease => {
                let url = legacy_release_url(chrome_major_version(chrome_version)?.as_str());
                let bytes = self.downloader.fetch(&url).await?;
                String::from_utf8(bytes)
                    .map(|value| value.trim().to_string())
                    .map_err(|e| ChromeToolError::DriverArchiveInvalid {
                        reason: format!("release metadata from '{url}' is not valid utf-8: {e}"),
                    })
            }
        }
    }

    fn find_cached_install(
        &self,
        chrome_version: &str,
        platform: &ChromePlatform,
    ) -> Result<Option<InstalledDriver>, ChromeToolError> {
        let major_prefix = format!("{}.", chrome_major_version(chrome_version)?);
        let exact_match = chrome_version.contains('.');
        let entries = std::fs::read_dir(&self.paths.patched).map_err(|e| {
            ChromeToolError::FileReadFailed {
                path: self.paths.patched.clone(),
                reason: e.to_string(),
            }
        })?;

        for entry in entries {
            let entry = entry.map_err(|e| ChromeToolError::FileReadFailed {
                path: self.paths.patched.clone(),
                reason: e.to_string(),
            })?;
            let file_name = entry.file_name();
            let file_name = file_name.to_string_lossy();
            let Some(version) = version_from_driver_filename(&file_name, platform) else {
                continue;
            };
            let version_matches = if exact_match {
                version == chrome_version
            } else {
                version.starts_with(&major_prefix)
            };
            if !version_matches {
                continue;
            }

            let patched_driver = entry.path();
            let original_driver = self.paths.driver.join(file_name.as_ref());
            if original_driver.is_file() && patched_driver.is_file() {
                return Ok(Some(InstalledDriver {
                    original_driver,
                    patched_driver,
                    driver_version: version,
                    cache_hit: true,
                }));
            }
        }

        Ok(None)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DriverVersionSource {
    ExactVersion,
    MilestoneJson,
    LegacyLatestRelease,
}

pub struct ReqwestDriverDownloader {
    client: Client,
}

impl ReqwestDriverDownloader {
    #[must_use]
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }
}

#[async_trait]
impl DriverDownloader for ReqwestDriverDownloader {
    async fn fetch(&self, url: &str) -> Result<Vec<u8>, ChromeToolError> {
        let response = self.client.get(url).send().await.map_err(|e| {
            ChromeToolError::DriverDownloadFailed {
                url: url.to_string(),
                reason: e.to_string(),
            }
        })?;
        let response =
            response
                .error_for_status()
                .map_err(|e| ChromeToolError::DriverDownloadFailed {
                    url: url.to_string(),
                    reason: e.to_string(),
                })?;
        let bytes = response
            .bytes()
            .await
            .map_err(|e| ChromeToolError::DriverDownloadFailed {
                url: url.to_string(),
                reason: e.to_string(),
            })?;
        Ok(bytes.to_vec())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChromePlatform {
    Linux64,
    MacArm64,
    MacX64,
    Win64,
}

impl ChromePlatform {
    fn detect() -> Result<Self, ChromeToolError> {
        match (std::env::consts::OS, std::env::consts::ARCH) {
            ("linux", "x86_64") => Ok(Self::Linux64),
            ("macos", "aarch64") => Ok(Self::MacArm64),
            ("macos", "x86_64") => Ok(Self::MacX64),
            ("windows", "x86_64") => Ok(Self::Win64),
            (os, arch) => Err(ChromeToolError::UnsupportedPlatform {
                os: os.to_string(),
                arch: arch.to_string(),
            }),
        }
    }

    fn archive_key(self) -> &'static str {
        match self {
            Self::Linux64 => "linux64",
            Self::MacArm64 => "mac-arm64",
            Self::MacX64 => "mac-x64",
            Self::Win64 => "win64",
        }
    }

    fn driver_binary_name(self) -> &'static str {
        match self {
            Self::Win64 => "chromedriver.exe",
            Self::Linux64 | Self::MacArm64 | Self::MacX64 => "chromedriver",
        }
    }
}

fn chrome_major_version(chrome_version: &str) -> Result<String, ChromeToolError> {
    chrome_version
        .split('.')
        .next()
        .unwrap_or(chrome_version)
        .parse::<u32>()
        .map(|value| value.to_string())
        .map_err(|e| ChromeToolError::DriverArchiveInvalid {
            reason: format!("invalid chrome milestone '{chrome_version}': {e}"),
        })
}

fn driver_version_source(chrome_version: &str) -> Result<DriverVersionSource, ChromeToolError> {
    let milestone = chrome_major_version(chrome_version)?
        .parse::<u32>()
        .map_err(|e| ChromeToolError::DriverArchiveInvalid {
            reason: format!("invalid chrome milestone '{chrome_version}': {e}"),
        })?;
    if chrome_version.contains('.') && milestone >= 114 {
        Ok(DriverVersionSource::ExactVersion)
    } else if milestone >= 114 {
        Ok(DriverVersionSource::MilestoneJson)
    } else {
        Ok(DriverVersionSource::LegacyLatestRelease)
    }
}

fn milestone_versions_url() -> &'static str {
    "https://googlechromelabs.github.io/chrome-for-testing/latest-versions-per-milestone.json"
}

fn legacy_release_url(chrome_major: &str) -> String {
    format!("https://googlechromelabs.github.io/chrome-for-testing/LATEST_RELEASE_{chrome_major}")
}

fn parse_milestone_version(body: &[u8], chrome_major: &str) -> Result<String, ChromeToolError> {
    let json = serde_json::from_slice::<serde_json::Value>(body).map_err(|e| {
        ChromeToolError::DriverArchiveInvalid {
            reason: format!("milestone metadata is not valid json: {e}"),
        }
    })?;
    json["milestones"][chrome_major]["version"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| ChromeToolError::DriverArchiveInvalid {
            reason: format!("milestone metadata does not contain version for '{chrome_major}'"),
        })
}

fn driver_archive_url(version: &str, platform: &ChromePlatform) -> String {
    let platform_key = platform.archive_key();
    format!(
        "https://storage.googleapis.com/chrome-for-testing-public/{version}/{platform_key}/chromedriver-{platform_key}.zip"
    )
}

fn driver_filename(version: &str, platform: &ChromePlatform) -> String {
    format!("chromedriver-{}-{version}", platform.archive_key())
        + if platform.driver_binary_name().ends_with(".exe") {
            ".exe"
        } else {
            ""
        }
}

fn extract_driver_binary(
    archive_bytes: &[u8],
    tmp_dir: &Path,
    platform: &ChromePlatform,
    output_path: &Path,
) -> Result<(), ChromeToolError> {
    let reader = Cursor::new(archive_bytes);
    let mut archive =
        ZipArchive::new(reader).map_err(|e| ChromeToolError::DriverArchiveInvalid {
            reason: e.to_string(),
        })?;
    let driver_name = platform.driver_binary_name();

    for index in 0..archive.len() {
        let mut file =
            archive
                .by_index(index)
                .map_err(|e| ChromeToolError::DriverArchiveInvalid {
                    reason: e.to_string(),
                })?;
        let matches_driver = Path::new(file.name())
            .file_name()
            .and_then(|name| name.to_str())
            == Some(driver_name);
        if !matches_driver {
            continue;
        }

        let mut bytes = Vec::new();
        std::io::copy(&mut file, &mut bytes).map_err(|e| {
            ChromeToolError::DriverArchiveInvalid {
                reason: e.to_string(),
            }
        })?;
        atomic_write_file(tmp_dir, output_path, &bytes)?;
        ensure_executable(output_path)?;
        return Ok(());
    }

    Err(ChromeToolError::DriverArchiveInvalid {
        reason: format!("archive does not contain '{driver_name}'"),
    })
}

fn atomic_write_file(tmp_dir: &Path, path: &Path, bytes: &[u8]) -> Result<(), ChromeToolError> {
    static NEXT_TEMP_FILE_ID: AtomicU64 = AtomicU64::new(0);

    let temp_id = NEXT_TEMP_FILE_ID.fetch_add(1, Ordering::Relaxed) + 1;
    let temp_path = tmp_dir.join(format!(
        ".{}.{}.{}.tmp",
        path.file_name()
            .and_then(|value| value.to_str())
            .unwrap_or("chrome"),
        std::process::id(),
        temp_id
    ));
    std::fs::write(&temp_path, bytes).map_err(|e| ChromeToolError::FileWriteFailed {
        path: temp_path.clone(),
        reason: e.to_string(),
    })?;
    if path.exists() {
        std::fs::remove_file(path).map_err(|e| ChromeToolError::FileWriteFailed {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;
    }
    std::fs::rename(&temp_path, path).map_err(|e| ChromeToolError::FileWriteFailed {
        path: path.to_path_buf(),
        reason: e.to_string(),
    })
}

fn ensure_executable(path: &Path) -> Result<(), ChromeToolError> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let permissions = std::fs::Permissions::from_mode(0o755);
        std::fs::set_permissions(path, permissions).map_err(|e| {
            ChromeToolError::FileWriteFailed {
                path: path.to_path_buf(),
                reason: e.to_string(),
            }
        })?;
    }

    Ok(())
}

fn repair_macos_code_signature(path: &Path) -> Result<(), ChromeToolError> {
    #[cfg(target_os = "macos")]
    {
        let output = std::process::Command::new("codesign")
            .args(["--force", "--sign", "-"])
            .arg(path)
            .output()
            .map_err(|e| ChromeToolError::DriverPatchFailed {
                path: path.to_path_buf(),
                reason: e.to_string(),
            })?;
        if !output.status.success() {
            let reason = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(ChromeToolError::DriverPatchFailed {
                path: path.to_path_buf(),
                reason: if reason.is_empty() {
                    format!("codesign exited with status {}", output.status)
                } else {
                    reason
                },
            });
        }
    }

    #[cfg(not(target_os = "macos"))]
    let _ = path;

    Ok(())
}

fn version_from_driver_filename(file_name: &str, platform: &ChromePlatform) -> Option<String> {
    let prefix = format!("chromedriver-{}-", platform.archive_key());
    let suffix = if platform.driver_binary_name().ends_with(".exe") {
        ".exe"
    } else {
        ""
    };
    let stripped = file_name.strip_prefix(&prefix)?;
    let stripped = if suffix.is_empty() {
        stripped
    } else {
        stripped.strip_suffix(suffix)?
    };
    Some(stripped.to_string())
}

struct InstallLockGuard {
    path: PathBuf,
}

impl InstallLockGuard {
    fn acquire(path: &Path) -> Result<Self, ChromeToolError> {
        Self::acquire_with_options(
            path,
            200,
            Duration::from_millis(25),
            Duration::from_secs(120),
        )
    }

    fn acquire_with_options(
        path: &Path,
        max_wait_attempts: usize,
        wait_interval: Duration,
        stale_after: Duration,
    ) -> Result<Self, ChromeToolError> {
        let mut attempts_remaining = max_wait_attempts.max(1);

        loop {
            match std::fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(path)
            {
                Ok(mut file) => {
                    use std::io::Write;

                    let _ = writeln!(file, "pid={}", std::process::id());
                    let _ = writeln!(
                        file,
                        "created_at={}",
                        SystemTime::now()
                            .duration_since(SystemTime::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_secs()
                    );
                    return Ok(Self {
                        path: path.to_path_buf(),
                    });
                }
                Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                    if recover_stale_lock(path, stale_after)? {
                        continue;
                    }
                    attempts_remaining -= 1;
                    if attempts_remaining == 0 {
                        break;
                    }
                    if !wait_interval.is_zero() {
                        thread::sleep(wait_interval);
                    }
                }
                Err(err) => {
                    return Err(ChromeToolError::FileWriteFailed {
                        path: path.to_path_buf(),
                        reason: err.to_string(),
                    });
                }
            }
        }

        Err(ChromeToolError::FileWriteFailed {
            path: path.to_path_buf(),
            reason: "timed out waiting for install lock".to_string(),
        })
    }
}

fn recover_stale_lock(path: &Path, stale_after: Duration) -> Result<bool, ChromeToolError> {
    let metadata = match std::fs::metadata(path) {
        Ok(metadata) => metadata,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(err) => {
            return Err(ChromeToolError::FileReadFailed {
                path: path.to_path_buf(),
                reason: err.to_string(),
            });
        }
    };
    let modified = metadata
        .modified()
        .map_err(|e| ChromeToolError::FileReadFailed {
            path: path.to_path_buf(),
            reason: e.to_string(),
        })?;
    let age = SystemTime::now()
        .duration_since(modified)
        .unwrap_or(Duration::ZERO);
    if age < stale_after {
        return Ok(false);
    }

    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(err) => Err(ChromeToolError::FileWriteFailed {
            path: path.to_path_buf(),
            reason: err.to_string(),
        }),
    }
}

impl Drop for InstallLockGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io::Write;
    use std::sync::Arc;
    use std::sync::Mutex as StdMutex;
    use std::time::Duration;

    use tempfile::tempdir;
    use zip::write::SimpleFileOptions;

    use super::*;

    #[test]
    fn ensure_directories_creates_expected_tree() {
        let home = tempdir().unwrap();
        let paths = ChromePaths::from_home(home.path());

        paths.ensure_directories().unwrap();

        assert!(paths.root.is_dir());
        assert!(paths.driver.is_dir());
        assert!(paths.patched.is_dir());
        assert!(paths.tmp.is_dir());
    }

    #[test]
    fn ensure_directories_returns_create_failed_error() {
        let home = tempdir().unwrap();
        let paths = ChromePaths::from_home(home.path());

        std::fs::create_dir_all(paths.root.parent().unwrap()).unwrap();
        std::fs::write(&paths.root, b"file-blocking-directory").unwrap();

        let err = paths.ensure_directories().unwrap_err();
        assert!(matches!(
            err,
            ChromeToolError::DirectoryCreateFailed { path, .. } if path == paths.root
        ));
    }

    #[derive(Default)]
    struct FakeDownloader {
        responses: HashMap<String, Vec<u8>>,
        requests: StdMutex<Vec<String>>,
    }

    impl FakeDownloader {
        fn with_zip_bytes(zip_bytes: Vec<u8>) -> Arc<Self> {
            let mut responses = HashMap::new();
            responses.insert(
                "latest-versions-per-milestone.json".to_string(),
                br#"{"milestones":{"124":{"version":"124.0.6367.91"}}}"#.to_vec(),
            );
            responses.insert("chromedriver-".to_string(), zip_bytes);
            Arc::new(Self {
                responses,
                requests: StdMutex::new(Vec::new()),
            })
        }
    }

    #[async_trait]
    impl DriverDownloader for FakeDownloader {
        async fn fetch(&self, url: &str) -> Result<Vec<u8>, ChromeToolError> {
            self.requests.lock().unwrap().push(url.to_string());
            self.responses
                .iter()
                .find_map(|(needle, body)| url.contains(needle).then(|| body.clone()))
                .ok_or_else(|| ChromeToolError::DriverDownloadFailed {
                    url: url.to_string(),
                    reason: "missing fake response".to_string(),
                })
        }
    }

    struct FailingDownloader;

    #[async_trait]
    impl DriverDownloader for FailingDownloader {
        async fn fetch(&self, url: &str) -> Result<Vec<u8>, ChromeToolError> {
            Err(ChromeToolError::DriverDownloadFailed {
                url: url.to_string(),
                reason: "network access should not be required".to_string(),
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

    fn fake_driver_zip_with_license_trap() -> Vec<u8> {
        let cursor = Cursor::new(Vec::new());
        let mut writer = zip::ZipWriter::new(cursor);
        let options = SimpleFileOptions::default();
        writer
            .start_file("chromedriver-mac-arm64/LICENSE.chromedriver", options)
            .unwrap();
        writer
            .write_all(b"license-text-that-must-not-be-installed")
            .unwrap();
        writer
            .start_file("chromedriver-mac-arm64/chromedriver", options)
            .unwrap();
        writer.write_all(b"real-mac-driver-binary").unwrap();
        writer.finish().unwrap().into_inner()
    }

    #[test]
    fn extract_driver_binary_ignores_license_entries() {
        let home = tempdir().unwrap();
        let output_path = home.path().join("chromedriver");

        extract_driver_binary(
            &fake_driver_zip_with_license_trap(),
            home.path(),
            &ChromePlatform::MacArm64,
            &output_path,
        )
        .unwrap();

        let installed_bytes = std::fs::read(&output_path).unwrap();
        assert_eq!(installed_bytes, b"real-mac-driver-binary");
    }

    #[tokio::test]
    async fn installer_writes_into_managed_directories() {
        let home = tempdir().unwrap();
        let paths = ChromePaths::from_home(home.path());
        let downloader = FakeDownloader::with_zip_bytes(fake_driver_zip());
        let installer = ChromeInstaller::new(paths.clone(), downloader);

        let install = installer.ensure_driver("124.0.6367.91").await.unwrap();

        assert!(install.original_driver.starts_with(&paths.driver));
        assert!(install.patched_driver.starts_with(&paths.patched));
        assert!(install.original_driver.is_file());
        assert!(install.patched_driver.is_file());
        assert_eq!(install.driver_version, "124.0.6367.91");
        assert!(!install.cache_hit);

        let patched_bytes = std::fs::read(&install.patched_driver).unwrap();
        assert!(
            patched_bytes
                .windows(22)
                .any(|window| window == b"XXXXXXXXXXXXXXXXXXXXXX")
        );
    }

    #[tokio::test]
    async fn installer_reuses_cached_driver_without_network() {
        let home = tempdir().unwrap();
        let paths = ChromePaths::from_home(home.path());
        let first_installer = ChromeInstaller::new(
            paths.clone(),
            FakeDownloader::with_zip_bytes(fake_driver_zip()),
        );
        let first = first_installer
            .ensure_driver("124.0.6367.91")
            .await
            .unwrap();

        let cached_installer = ChromeInstaller::new(paths, Arc::new(FailingDownloader));
        let second = cached_installer
            .ensure_driver("124.0.6367.91")
            .await
            .unwrap();

        assert_eq!(second.original_driver, first.original_driver);
        assert_eq!(second.patched_driver, first.patched_driver);
        assert_eq!(second.driver_version, first.driver_version);
        assert!(!first.cache_hit);
        assert!(second.cache_hit);
    }

    #[test]
    fn find_installed_driver_returns_none_without_creating_directories() {
        let home = tempdir().unwrap();
        let paths = ChromePaths::from_home(home.path());
        let installer = ChromeInstaller::new(paths.clone(), FakeDownloader::with_zip_bytes(vec![]));

        let install = installer.find_installed_driver("124.0.6367.91").unwrap();

        assert!(install.is_none());
        assert!(!paths.root.exists());
    }

    #[tokio::test]
    async fn resolve_driver_version_uses_exact_browser_version_for_modern_chrome() {
        let home = tempdir().unwrap();
        let paths = ChromePaths::from_home(home.path());
        let downloader = Arc::new(FakeDownloader {
            responses: HashMap::new(),
            requests: StdMutex::new(Vec::new()),
        });
        let installer = ChromeInstaller::new(paths, downloader.clone());

        let version = installer
            .resolve_driver_version("124.0.6367.91")
            .await
            .unwrap();

        assert_eq!(version, "124.0.6367.91");
        assert!(downloader.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn resolve_driver_version_uses_legacy_release_endpoint_for_older_chrome() {
        let home = tempdir().unwrap();
        let paths = ChromePaths::from_home(home.path());
        let downloader = Arc::new(FakeDownloader {
            responses: HashMap::from([(
                "LATEST_RELEASE_113".to_string(),
                b"113.0.5672.63".to_vec(),
            )]),
            requests: StdMutex::new(Vec::new()),
        });
        let installer = ChromeInstaller::new(paths, downloader.clone());

        let version = installer.resolve_driver_version("113").await.unwrap();

        assert_eq!(version, "113.0.5672.63");
        assert_eq!(downloader.requests.lock().unwrap().len(), 1);
        assert!(downloader.requests.lock().unwrap()[0].contains("LATEST_RELEASE_113"));
    }

    #[test]
    fn platform_archive_keys_match_primary_targets() {
        assert_eq!(ChromePlatform::MacArm64.archive_key(), "mac-arm64");
        assert_eq!(ChromePlatform::Win64.archive_key(), "win64");
    }

    #[test]
    fn install_lock_reclaims_stale_lock_file() {
        let home = tempdir().unwrap();
        let lock_path = home.path().join(".install.lock");
        std::fs::write(&lock_path, b"stale-lock").unwrap();

        {
            let _guard = InstallLockGuard::acquire_with_options(
                &lock_path,
                1,
                Duration::from_millis(0),
                Duration::ZERO,
            )
            .unwrap();
            assert!(lock_path.exists());
        }

        assert!(!lock_path.exists());
    }
}
