//! CLI shared library for argusclaw and argusclaw-dev binaries.

use std::collections::HashMap;
use std::env;
use std::path::PathBuf;

use claw::db::llm::LlmProviderSummary;
use claw::llm::LlmStreamEvent;

pub mod provider;

#[cfg(feature = "dev")]
pub mod dev;

// ---------------------------------------------------------------------------
// Database path resolution
// ---------------------------------------------------------------------------

/// Resolve database path based on CLI mode (production vs development).
///
/// # Production (argusclaw)
/// - Default: `~/.argusclaw/sqlite.db`
/// - Override: `ARGUSCLAW_DB` environment variable
///
/// # Development (argusclaw-dev)
/// - Default: `./tmp/argusclaw-dev.db`
/// - Override: `ARGUSCLAW_DEV_DB` environment variable
pub fn resolve_db_path(is_dev: bool) -> PathBuf {
    let (env_var, default_path) = if is_dev {
        ("ARGUSCLAW_DEV_DB", {
            let cwd = env::current_dir().expect("failed to resolve current working directory");
            let tmp_dir = cwd.join("tmp");
            std::fs::create_dir_all(&tmp_dir).expect("failed to create tmp directory");
            tmp_dir.join("argusclaw-dev.db")
        })
    } else {
        ("ARGUSCLAW_DB", {
            let home = dirs::home_dir().expect("failed to resolve home directory");
            let data_dir = home.join(".argusclaw");
            std::fs::create_dir_all(&data_dir).expect("failed to create .argusclaw directory");
            data_dir.join("sqlite.db")
        })
    };

    if let Ok(value) = env::var(env_var) {
        if let Some(stripped) = value.strip_prefix("sqlite:") {
            PathBuf::from(stripped)
        } else {
            PathBuf::from(value)
        }
    } else {
        default_path
    }
}

/// Convert database path to connection URL format.
pub fn db_path_to_url(path: &std::path::Path) -> String {
    format!("sqlite:{}", path.display())
}

// ---------------------------------------------------------------------------
// Stream rendering (shared between production and dev CLIs)
// ---------------------------------------------------------------------------

/// State for rendering LLM stream events.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct StreamRenderState {
    pub reasoning_started: bool,
    pub summary_started: bool,
}

/// Render a single stream event to output string.
pub fn render_stream_event(
    state: &mut StreamRenderState,
    event: &LlmStreamEvent,
) -> Option<String> {
    match event {
        LlmStreamEvent::ReasoningDelta { delta } if !delta.is_empty() => {
            let mut output = String::new();
            if !state.reasoning_started {
                output.push_str("[Reasoning] ");
                state.reasoning_started = true;
            }
            output.push_str(delta);
            Some(output)
        }
        LlmStreamEvent::ContentDelta { delta } if !delta.is_empty() => {
            let mut output = String::new();
            if !state.summary_started {
                if state.reasoning_started {
                    output.push('\n');
                }
                output.push_str("[Summary] ");
                state.summary_started = true;
            }
            output.push_str(delta);
            Some(output)
        }
        _ => None,
    }
}

/// Finish stream output with trailing newline if needed.
pub fn finish_stream_output(state: &StreamRenderState) -> Option<&'static str> {
    (state.reasoning_started || state.summary_started).then_some("\n")
}

/// Render stream output for a complete set of events (for testing).
#[cfg(test)]
pub fn render_stream_output(events: &[LlmStreamEvent]) -> String {
    let mut state = StreamRenderState::default();
    let mut output = String::new();

    for event in events {
        if let Some(chunk) = render_stream_event(&mut state, event) {
            output.push_str(&chunk);
        }
    }

    if let Some(suffix) = finish_stream_output(&state) {
        output.push_str(suffix);
    }

    output
}

// ---------------------------------------------------------------------------
// Display types
// ---------------------------------------------------------------------------

/// Provider record for display (without sensitive data).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderDisplayRecord {
    pub id: String,
    pub display_name: String,
    pub kind: String,
    pub base_url: String,
    pub model: String,
    pub is_default: bool,
    pub extra_headers: HashMap<String, String>,
}

impl From<LlmProviderSummary> for ProviderDisplayRecord {
    fn from(value: LlmProviderSummary) -> Self {
        Self {
            id: value.id.to_string(),
            display_name: value.display_name,
            kind: value.kind.to_string(),
            base_url: value.base_url,
            model: value.model,
            is_default: value.is_default,
            extra_headers: value.extra_headers,
        }
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_db_path_production_defaults_to_home() {
        let path = resolve_db_path(false);
        assert!(path.to_string_lossy().ends_with("sqlite.db"));
        assert!(path.to_string_lossy().contains(".argusclaw"));
    }

    #[test]
    fn resolve_db_path_dev_defaults_to_tmp() {
        let path = resolve_db_path(true);
        assert!(path.to_string_lossy().ends_with("argusclaw-dev.db"));
        assert!(path.to_string_lossy().contains("tmp"));
    }

    #[test]
    fn db_path_to_url_formats_correctly() {
        let path = PathBuf::from("/path/to/db.sqlite");
        let url = db_path_to_url(&path);
        assert_eq!(url, "sqlite:/path/to/db.sqlite");
    }

    #[test]
    fn render_stream_output_formats_reasoning_and_summary_sections() {
        let output = render_stream_output(&[
            LlmStreamEvent::ReasoningDelta {
                delta: "step 1".to_string(),
            },
            LlmStreamEvent::ReasoningDelta {
                delta: " -> step 2".to_string(),
            },
            LlmStreamEvent::ContentDelta {
                delta: "final answer".to_string(),
            },
        ]);

        assert_eq!(
            output,
            "[Reasoning] step 1 -> step 2\n[Summary] final answer\n"
        );
    }
}
