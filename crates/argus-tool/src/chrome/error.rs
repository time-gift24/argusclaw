use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum ChromeToolError {
    #[error("invalid arguments: {reason}")]
    InvalidArguments { reason: String },

    #[error("missing required field '{field}' for action '{action}'")]
    MissingRequiredField { action: String, field: &'static str },

    #[error("action '{action}' is not allowed")]
    ActionNotAllowed { action: String },

    #[error("session not found: {session_id}")]
    SessionNotFound { session_id: String },

    #[error("failed to create directory '{path:?}': {reason}")]
    DirectoryCreateFailed { path: PathBuf, reason: String },

    #[error("failed to download chrome driver from '{url}': {reason}")]
    DriverDownloadFailed { url: String, reason: String },

    #[error("chrome is not installed")]
    ChromeNotInstalled,

    #[error(
        "matching chromedriver for chrome '{browser_version}' is not installed; run chrome with action: {suggested_action}"
    )]
    DriverNotInstalled {
        browser_version: String,
        suggested_action: String,
    },

    #[error("chrome driver installation is unavailable for this backend")]
    InstallUnavailable,

    #[error("unsupported chrome platform '{os}/{arch}'")]
    UnsupportedPlatform { os: String, arch: String },

    #[error("failed to detect chrome version from '{path:?}': {reason}")]
    ChromeVersionDetectFailed { path: PathBuf, reason: String },

    #[error("invalid chrome driver archive: {reason}")]
    DriverArchiveInvalid { reason: String },

    #[error("failed to patch chrome driver '{path:?}': {reason}")]
    DriverPatchFailed { path: PathBuf, reason: String },

    #[error("failed to start chrome driver: {reason}")]
    DriverStartFailed { reason: String },

    #[error("failed to read file '{path:?}': {reason}")]
    FileReadFailed { path: PathBuf, reason: String },

    #[error("failed to write file '{path:?}': {reason}")]
    FileWriteFailed { path: PathBuf, reason: String },

    #[error("failed to navigate to '{url}': {reason}")]
    NavigationFailed { url: String, reason: String },

    #[error("failed to read page state: {reason}")]
    PageReadFailed { reason: String },

    #[error("failed to shut down chrome session: {reason}")]
    SessionShutdownFailed { reason: String },

    #[error("failed to interact with element: {reason}")]
    InteractionFailed { reason: String },
}
