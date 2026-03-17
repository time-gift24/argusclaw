use std::collections::HashMap;
use std::fmt;
use std::str::FromStr;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::db::DbError;

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
    type Err = DbError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "openai-compatible" => Ok(Self::OpenAiCompatible),
            _ => Err(DbError::InvalidProviderKind {
                kind: value.to_string(),
            }),
        }
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSecretStatus {
    Ready,
    RequiresReentry,
}

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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmProviderSummary {
    pub id: LlmProviderId,
    pub kind: LlmProviderKind,
    pub display_name: String,
    pub base_url: String,
    pub models: Vec<String>,
    pub default_model: String,
    pub is_default: bool,
    pub extra_headers: HashMap<String, String>,
    pub secret_status: ProviderSecretStatus,
}

impl From<LlmProviderRecord> for LlmProviderSummary {
    fn from(record: LlmProviderRecord) -> Self {
        Self {
            id: record.id,
            kind: record.kind,
            display_name: record.display_name,
            base_url: record.base_url,
            models: record.models,
            default_model: record.default_model,
            is_default: record.is_default,
            extra_headers: record.extra_headers,
            secret_status: record.secret_status,
        }
    }
}

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

#[async_trait]
pub trait LlmProviderRepository: Send + Sync {
    /// Upserts a provider record. Returns the provider ID (newly generated for inserts, or the
    /// existing ID for updates).
    async fn upsert_provider(&self, record: &LlmProviderRecord) -> Result<LlmProviderId, DbError>;

    async fn delete_provider(&self, id: &LlmProviderId) -> Result<bool, DbError>;

    async fn set_default_provider(&self, id: &LlmProviderId) -> Result<(), DbError>;

    async fn get_provider(&self, id: &LlmProviderId) -> Result<Option<LlmProviderRecord>, DbError>;

    async fn get_provider_summary(
        &self,
        id: &LlmProviderId,
    ) -> Result<Option<LlmProviderSummary>, DbError>;

    async fn list_providers(&self) -> Result<Vec<LlmProviderSummary>, DbError>;

    async fn get_default_provider(&self) -> Result<Option<LlmProviderRecord>, DbError>;
}

#[cfg(test)]
mod multi_model_tests {
    use super::*;
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
    fn llm_provider_summary_has_models_and_default_model() {
        let summary = LlmProviderSummary {
            id: LlmProviderId::new(1),
            kind: LlmProviderKind::OpenAiCompatible,
            display_name: "Test".to_string(),
            base_url: "https://api.example.com/v1".to_string(),
            models: vec!["o3".to_string()],
            default_model: "o3".to_string(),
            is_default: false,
            extra_headers: HashMap::new(),
            secret_status: ProviderSecretStatus::Ready,
        };

        assert_eq!(summary.models, vec!["o3"]);
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    use super::{ProviderTestResult, ProviderTestStatus};

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
