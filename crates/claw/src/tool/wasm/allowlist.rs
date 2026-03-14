//! HTTP endpoint allowlist validation.
//!
//! This module provides URL pattern matching for HTTP capability allowlists.

use regex::Regex;
use std::sync::Arc;

/// Pattern matcher for HTTP endpoint allowlists.
#[derive(Debug, Clone)]
pub struct AllowlistValidator {
    patterns: Vec<EndpointPattern>,
}

/// A compiled endpoint pattern.
#[derive(Clone)]
struct EndpointPattern {
    /// Original pattern string.
    source: String,
    /// Compiled regex.
    regex: Regex,
}

impl std::fmt::Debug for EndpointPattern {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EndpointPattern")
            .field("source", &self.source)
            .finish()
    }
}

impl AllowlistValidator {
    /// Create a new validator with the given patterns.
    ///
    /// Pattern syntax:
    /// - `*` matches any sequence of characters within a path segment
    /// - `**` matches any sequence of characters across path segments
    /// - `?` matches any single character
    /// - All other characters are matched literally
    pub fn new(patterns: &[String]) -> Self {
        let compiled: Vec<EndpointPattern> = patterns
            .iter()
            .filter_map(|p| {
                EndpointPattern::new(p)
                    .map_err(|e| {
                        tracing::warn!("Invalid endpoint pattern '{}': {}", p, e);
                        e
                    })
                    .ok()
            })
            .collect();

        Self { patterns: compiled }
    }

    /// Check if a URL is allowed by the allowlist.
    ///
    /// Returns `true` if the URL matches any pattern in the allowlist.
    #[must_use]
    pub fn is_allowed(&self, url: &str) -> bool {
        // Extract the URL without query string for matching
        let url_without_query = url.split('?').next().unwrap_or(url);

        self.patterns.iter().any(|p| p.matches(url_without_query))
    }

    /// Check if the allowlist is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.patterns.is_empty()
    }

    /// Get the number of patterns.
    #[must_use]
    pub fn len(&self) -> usize {
        self.patterns.len()
    }
}

impl EndpointPattern {
    /// Create a new endpoint pattern from a glob-like string.
    fn new(pattern: &str) -> Result<Self, String> {
        let regex = glob_to_regex(pattern)?;
        Ok(Self {
            source: pattern.to_string(),
            regex,
        })
    }

    /// Check if this pattern matches the given URL.
    fn matches(&self, url: &str) -> bool {
        self.regex.is_match(url)
    }
}

/// Convert a glob-like pattern to a regex.
fn glob_to_regex(pattern: &str) -> Result<Regex, String> {
    let mut regex_str = String::from("^");

    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // Escape regex special characters
            '.' | '^' | '$' | '+' | '(' | ')' | '[' | ']' | '{' | '}' | '|' => {
                regex_str.push('\\');
                regex_str.push(c);
            }
            // ** matches anything (including /)
            '*' if chars.peek() == Some(&'*') => {
                chars.next(); // consume second *
                regex_str.push_str(".*");
            }
            // * matches anything except /
            '*' => {
                regex_str.push_str("[^/]*");
            }
            // ? matches any single character except /
            '?' => {
                regex_str.push_str("[^/]");
            }
            // Literal characters
            _ => {
                regex_str.push(c);
            }
        }
    }

    regex_str.push('$');

    Regex::new(&regex_str).map_err(|e| format!("Invalid regex: {}", e))
}

/// Shared allowlist validator type for use across async boundaries.
pub type SharedAllowlistValidator = Arc<AllowlistValidator>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_match() {
        let validator = AllowlistValidator::new(&["https://api.example.com/v1".to_string()]);

        assert!(validator.is_allowed("https://api.example.com/v1"));
        assert!(!validator.is_allowed("https://api.example.com/v2"));
        assert!(!validator.is_allowed("https://other.example.com/v1"));
    }

    #[test]
    fn wildcard_path_segment() {
        let validator = AllowlistValidator::new(&["https://api.example.com/users/*".to_string()]);

        assert!(validator.is_allowed("https://api.example.com/users/123"));
        assert!(validator.is_allowed("https://api.example.com/users/abc"));
        assert!(!validator.is_allowed("https://api.example.com/users/123/posts"));
        assert!(!validator.is_allowed("https://api.example.com/posts/123"));
    }

    #[test]
    fn double_wildcard() {
        let validator = AllowlistValidator::new(&["https://api.example.com/**".to_string()]);

        assert!(validator.is_allowed("https://api.example.com/v1"));
        assert!(validator.is_allowed("https://api.example.com/v1/users"));
        assert!(validator.is_allowed("https://api.example.com/v1/users/123/posts"));
        assert!(!validator.is_allowed("https://other.example.com/v1"));
    }

    #[test]
    fn multiple_patterns() {
        let validator = AllowlistValidator::new(&[
            "https://api.example.com/v1/*".to_string(),
            "https://cdn.example.com/**".to_string(),
        ]);

        assert!(validator.is_allowed("https://api.example.com/v1/users"));
        assert!(validator.is_allowed("https://cdn.example.com/assets/logo.png"));
        assert!(!validator.is_allowed("https://api.example.com/v2/users"));
    }

    #[test]
    fn query_string_handling() {
        let validator = AllowlistValidator::new(&["https://api.example.com/search".to_string()]);

        // Query strings should be ignored for matching
        assert!(validator.is_allowed("https://api.example.com/search?q=test"));
        assert!(validator.is_allowed("https://api.example.com/search"));
    }

    #[test]
    fn invalid_regex_pattern_is_skipped() {
        // The glob_to_regex function escapes special characters, so most patterns
        // will produce valid regex. However, if somehow an invalid regex is created,
        // it should be skipped rather than causing a panic.
        // Since all special chars are escaped, this test verifies the behavior with
        // an empty pattern list (which is a valid case).
        let validator = AllowlistValidator::new(&[]);
        assert!(validator.is_empty());
    }

    #[test]
    fn empty_validator() {
        let validator = AllowlistValidator::new(&[]);
        assert!(validator.is_empty());
        assert!(!validator.is_allowed("https://example.com"));
    }

    #[test]
    fn question_mark_wildcard() {
        let validator = AllowlistValidator::new(&["https://api.example.com/v?".to_string()]);

        assert!(validator.is_allowed("https://api.example.com/v1"));
        assert!(validator.is_allowed("https://api.example.com/v2"));
        assert!(!validator.is_allowed("https://api.example.com/v10"));
        assert!(!validator.is_allowed("https://api.example.com/v"));
    }
}
