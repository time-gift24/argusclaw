//! Configuration for committed turn-log persistence.

use std::path::PathBuf;

/// Configuration shared by thread runtimes when persisting committed turn logs.
#[derive(Debug, Clone)]
pub struct TraceConfig {
    /// Whether committed turn-log persistence is enabled.
    pub enabled: bool,
    /// Explicit directory for this thread node.
    pub thread_base_dir: PathBuf,
    /// Optional model name persisted into turn metadata.
    pub model: Option<String>,
}

impl TraceConfig {
    /// Create a new TraceConfig.
    #[must_use]
    pub fn new(enabled: bool, thread_base_dir: PathBuf) -> Self {
        Self {
            enabled,
            thread_base_dir,
            model: None,
        }
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
            thread_base_dir: PathBuf::new(),
            model: None,
        }
    }
}

impl Default for TraceConfig {
    fn default() -> Self {
        Self::disabled()
    }
}
