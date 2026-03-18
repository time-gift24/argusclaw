//! Provider domain types for LLM management.
//!
//! These types are shared between:
//! - `argus-llm` (provider management)
//! - `claw` (SQLite repository implementation)
//! - `cli`/`desktop` (consumer applications)

use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ============================================================================
// ID Types
// ============================================================================

/// Unique identifier for an LLM provider (auto-increment INTEGER).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LlmProviderId(i64);

impl LlmProviderId {
    /// Creates a new provider ID from a database-generated i64.
    #[must_use]
    pub const fn new(id: i64) -> Self {
        Self(id)
    }

    /// Returns the underlying i64 value.
    #[must_use]
    pub const fn into_inner(self) -> i64 {
        self.0
    }
}

impl From<i64> for LlmProviderId {
    fn from(id: i64) -> Self {
        Self::new(id)
    }
}

impl From<LlmProviderId> for i64 {
    fn from(id: LlmProviderId) -> Self {
        id.into_inner()
    }
}

impl fmt::Display for LlmProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ============================================================================
// Provider Kind
// ============================================================================

/// Supported LLM provider kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmProviderKind {
    OpenAiCompatible,
}

impl LlmProviderKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::OpenAiCompatible => "openai-compatible",
        }
    }
}

impl fmt::Display for LlmProviderKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl FromStr for LlmProviderKind {
    type Err = LlmProviderKindParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "openai-compatible" => Ok(Self::OpenAiCompatible),
            _ => Err(LlmProviderKindParseError {
                kind: value.to_string(),
            }),
        }
    }
}

/// Error returned when parsing an invalid provider kind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmProviderKindParseError {
    pub kind: String,
}

impl fmt::Display for LlmProviderKindParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid provider kind: {}", self.kind)
    }
}

impl std::error::Error for LlmProviderKindParseError {}

// ============================================================================
// Secret Types
// ============================================================================

/// A string that hides its contents in debug output.
#[derive(Clone, PartialEq, Eq)]
pub struct SecretString(String);

impl SecretString {
    #[must_use]
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    #[must_use]
    pub fn expose_secret(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for SecretString {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SecretString(REDACTED)")
    }
}

/// Status of a provider's secret (API key).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSecretStatus {
    /// Secret is ready for use.
    Ready,
    /// Secret requires re-entry (e.g., after key material change).
    RequiresReentry,
}

// ============================================================================
// Provider Records
// ============================================================================

/// Full provider record including sensitive data.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmProviderRecord {
    pub id: LlmProviderId,
    pub kind: LlmProviderKind,
    pub display_name: String,
    pub base_url: String,
    pub api_key: SecretString,
    pub models: Vec<String>,
    pub default_model: String,
    pub is_default: bool,
    pub extra_headers: HashMap<String, String>,
    pub secret_status: ProviderSecretStatus,
}

// ============================================================================
// Provider Test Types
// ============================================================================

/// Status of a provider connection test.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderTestStatus {
    Success,
    AuthFailed,
    ModelNotAvailable,
    RateLimited,
    RequestFailed,
    InvalidResponse,
    ProviderNotFound,
    UnsupportedProviderKind,
}

/// Result of a provider connection test.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderTestResult {
    pub provider_id: String,
    pub model: String,
    pub base_url: String,
    pub checked_at: DateTime<Utc>,
    pub latency_ms: u64,
    pub status: ProviderTestStatus,
    pub message: String,
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use serde_json::json;
    use std::collections::HashMap;

    #[test]
    fn llm_provider_record_has_models_and_default_model() {
        let record = LlmProviderRecord {
            id: LlmProviderId::new(1),
            kind: LlmProviderKind::OpenAiCompatible,
            display_name: "Test".to_string(),
            base_url: "https://api.example.com/v1".to_string(),
            api_key: SecretString::new("sk-test"),
            models: vec!["gpt-4.1".to_string(), "gpt-4.1-mini".to_string()],
            default_model: "gpt-4.1".to_string(),
            is_default: true,
            extra_headers: HashMap::new(),
            secret_status: ProviderSecretStatus::Ready,
        };

        assert_eq!(record.models.len(), 2);
        assert_eq!(record.default_model, "gpt-4.1");
    }

    #[test]
    fn provider_test_result_serializes_with_stable_shape() {
        let result = ProviderTestResult {
            provider_id: "openai".to_string(),
            model: "gpt-4.1".to_string(),
            base_url: "https://api.example.com/v1".to_string(),
            checked_at: Utc
                .with_ymd_and_hms(2026, 3, 16, 12, 0, 0)
                .single()
                .expect("timestamp should be valid"),
            latency_ms: 42,
            status: ProviderTestStatus::ModelNotAvailable,
            message: "Model gpt-4.1 not available on provider openai-compatible".to_string(),
        };

        let serialized = serde_json::to_value(result).expect("result should serialize");

        assert_eq!(
            serialized,
            json!({
                "provider_id": "openai",
                "model": "gpt-4.1",
                "base_url": "https://api.example.com/v1",
                "checked_at": "2026-03-16T12:00:00Z",
                "latency_ms": 42,
                "status": "model_not_available",
                "message": "Model gpt-4.1 not available on provider openai-compatible",
            })
        );
    }
}
