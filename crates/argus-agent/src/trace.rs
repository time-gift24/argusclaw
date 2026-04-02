//! Configuration for committed turn-log persistence.

use std::path::PathBuf;

/// Configuration shared by thread runtimes when persisting committed turn logs.
#[derive(Debug, Clone)]
pub struct TraceConfig {
    /// Whether committed turn-log persistence is enabled.
    pub enabled: bool,
    /// Root directory where turn logs are written.
    pub trace_dir: PathBuf,
    /// Session ID (included in path: `{trace_dir}/{session_id}/{thread_id}/`).
    pub session_id: Option<argus_protocol::SessionId>,
    /// Optional model name persisted into turn metadata.
    pub model: Option<String>,
}

impl TraceConfig {
    /// Create a new TraceConfig.
    #[must_use]
    pub fn new(enabled: bool, trace_dir: PathBuf) -> Self {
        Self {
            enabled,
            trace_dir,
            session_id: None,
            model: None,
        }
    }

    /// Set the session ID for the turn-log path.
    #[must_use]
    pub fn with_session_id(mut self, session_id: argus_protocol::SessionId) -> Self {
        self.session_id = Some(session_id);
        self
    }

    /// Set the model name persisted into turn metadata.
    #[must_use]
    pub fn with_model(mut self, model: Option<String>) -> Self {
        self.model = model;
        self
    }

    /// Create a disabled TraceConfig.
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            trace_dir: PathBuf::new(),
            session_id: None,
            model: None,
        }
    }
}

impl Default for TraceConfig {
    fn default() -> Self {
        Self::disabled()
    }
}
