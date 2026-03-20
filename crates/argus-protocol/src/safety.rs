//! Safety layer for tool output sanitization.
//!
//! This module provides output truncation to prevent excessive token consumption
//! when tool outputs are sent to the LLM.

use std::env;

/// Configuration for safety limits.
#[derive(Debug, Clone)]
pub struct SafetyConfig {
    /// Maximum output length in bytes.
    pub max_output_length: u64,
}

impl SafetyConfig {
    /// Create a new SafetyConfig with default limits.
    ///
    /// Default: 100KB (102400 bytes)
    pub fn new() -> Self {
        Self {
            max_output_length: 100 * 1024, // 100KB default
        }
    }

    /// Create a SafetyConfig from environment variable or default.
    ///
    /// Reads `ARGUS_MAX_TOOL_OUTPUT_LENGTH` env var if set.
    pub fn from_env() -> Self {
        let max_output_length = env::var("ARGUS_MAX_TOOL_OUTPUT_LENGTH")
            .ok()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(100 * 1024); // 100KB default

        Self { max_output_length }
    }
}

impl Default for SafetyConfig {
    fn default() -> Self {
        Self::new()
    }
}

/// Warning for truncated tool output.
#[derive(Debug, Clone)]
pub struct OutputWarning {
    /// Warning pattern identifier.
    pub pattern: &'static str,
    /// Original content length in bytes.
    pub original_len: usize,
    /// Length after truncation.
    pub truncated_len: usize,
    /// The applied limit.
    pub limit: u64,
}

/// Find the nearest character boundary to truncate at safely.
///
/// UTF-8 characters can be 1-4 bytes. This function ensures we don't
/// split a multi-byte character by finding the nearest byte position
/// that is also a valid UTF-8 character boundary.
fn find_char_boundary(s: &str, max_bytes: usize) -> usize {
    if s.len() <= max_bytes {
        return s.len();
    }

    // Find the last valid UTF-8 character boundary before max_bytes
    let mut pos = max_bytes;
    while pos > 0 && !s.is_char_boundary(pos) {
        pos -= 1;
    }

    // If we somehow ended up at 0, fall back to max_bytes (shouldn't happen
    // if max_bytes > 0 and input is valid UTF-8)
    if pos == 0 {
        pos = max_bytes.saturating_sub(1);
    }

    pos
}

/// Sanitize tool output by truncating if it exceeds the configured limit.
///
/// Keeps the START of the output (preserves JSON structure at the beginning).
/// Returns the sanitized content and an optional warning if truncation occurred.
///
/// # Arguments
///
/// * `content` - The tool output content to sanitize
/// * `config` - Safety configuration with max length limit
///
/// # Returns
///
/// A tuple of (sanitized_content, optional_warning)
pub fn sanitize_tool_output(
    content: &str,
    config: &SafetyConfig,
) -> (String, Option<OutputWarning>) {
    let original_len = content.len();

    if original_len as u64 <= config.max_output_length {
        return (content.to_string(), None);
    }

    let max_bytes = config.max_output_length as usize;
    let char_boundary = find_char_boundary(content, max_bytes);

    // Truncate at character boundary
    let truncated = content[..char_boundary].to_string();
    let truncated_len = truncated.len();

    tracing::warn!(
        original_len = original_len,
        truncated_len = truncated_len,
        limit = config.max_output_length,
        pattern = "output_too_large",
        "Tool output was truncated"
    );

    let warning = OutputWarning {
        pattern: "output_too_large",
        original_len,
        truncated_len,
        limit: config.max_output_length,
    };

    (truncated, Some(warning))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_no_truncation_under_limit() {
        let config = SafetyConfig {
            max_output_length: 100,
        };
        let content = "Hello, World!";
        let (result, warning) = sanitize_tool_output(content, &config);

        assert_eq!(result, content);
        assert!(warning.is_none());
    }

    #[test]
    fn test_truncation_at_boundary() {
        let config = SafetyConfig {
            max_output_length: 10,
        };
        let content = "Hello, World! This is a test.";
        let (result, warning) = sanitize_tool_output(content, &config);

        assert!(result.len() <= 10);
        assert!(warning.is_some());
        let w = warning.unwrap();
        assert_eq!(w.pattern, "output_too_large");
        assert_eq!(w.original_len, content.len());
        assert_eq!(w.truncated_len, result.len());
        assert_eq!(w.limit, 10);
    }

    #[test]
    fn test_char_boundary_safety() {
        let config = SafetyConfig {
            max_output_length: 5,
        };
        // "Hello" is 5 bytes, "🎉" is 4 bytes in UTF-8
        // If we cut at byte 5, we might split the emoji
        let content = "Hello🎉";
        let (result, warning) = sanitize_tool_output(content, &config);

        // Should not panic and should produce valid UTF-8
        assert!(std::str::from_utf8(result.as_bytes()).is_ok());
        assert!(warning.is_some());

        // The result should be "Hello" (5 bytes, all ASCII)
        assert_eq!(result, "Hello");
    }

    #[test]
    fn test_default_config() {
        let config = SafetyConfig::new();
        assert_eq!(config.max_output_length, 100 * 1024);
    }

    #[test]
    fn test_multibyte_char_truncation() {
        let config = SafetyConfig {
            max_output_length: 6,
        };
        // "H🎉" = 1 + 4 = 5 bytes
        // If limit is 6, we should get "H🎉" (5 bytes)
        let content = "H🎉";
        let (result, _warning) = sanitize_tool_output(content, &config);
        assert_eq!(result, "H🎉");

        // "Hi🎉" = 2 + 4 = 6 bytes - exactly at limit
        let content2 = "Hi🎉";
        let (result2, warning2) = sanitize_tool_output(content2, &config);
        assert_eq!(result2, "Hi🎉");
        assert!(warning2.is_none());

        // "Hi🎉!" = 3 + 4 = 7 bytes - should truncate to "Hi" (2 bytes)
        // because at byte 6 we might be in middle of emoji
        let content3 = "Hi🎉!";
        let (result3, warning3) = sanitize_tool_output(content3, &config);
        // Should be valid UTF-8
        assert!(std::str::from_utf8(result3.as_bytes()).is_ok());
        assert!(warning3.is_some());
    }

    #[test]
    fn test_empty_content() {
        let config = SafetyConfig {
            max_output_length: 100,
        };
        let content = "";
        let (result, warning) = sanitize_tool_output(content, &config);

        assert_eq!(result, "");
        assert!(warning.is_none());
    }

    #[test]
    fn test_exactly_at_limit() {
        let config = SafetyConfig {
            max_output_length: 5,
        };
        let content = "Hello";
        let (result, warning) = sanitize_tool_output(content, &config);

        assert_eq!(result, "Hello");
        assert!(warning.is_none());
    }
}
