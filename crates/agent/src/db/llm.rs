use std::fmt;
use std::str::FromStr;

use async_trait::async_trait;

use crate::db::DbError;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct LlmProviderId(String);

impl LlmProviderId {
    #[must_use]
    pub fn new(id: impl Into<String>) -> Self {
        Self(id.into())
    }
}

impl AsRef<str> for LlmProviderId {
    fn as_ref(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for LlmProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_ref())
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmProviderRecord {
    pub id: LlmProviderId,
    pub kind: LlmProviderKind,
    pub display_name: String,
    pub base_url: String,
    pub api_key: SecretString,
    pub model: String,
    pub is_default: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LlmProviderSummary {
    pub id: LlmProviderId,
    pub kind: LlmProviderKind,
    pub display_name: String,
    pub base_url: String,
    pub model: String,
    pub is_default: bool,
}

impl From<LlmProviderRecord> for LlmProviderSummary {
    fn from(record: LlmProviderRecord) -> Self {
        Self {
            id: record.id,
            kind: record.kind,
            display_name: record.display_name,
            base_url: record.base_url,
            model: record.model,
            is_default: record.is_default,
        }
    }
}

#[async_trait]
pub trait LlmProviderRepository: Send + Sync {
    async fn upsert_provider(&self, record: &LlmProviderRecord) -> Result<(), DbError>;

    async fn set_default_provider(&self, id: &LlmProviderId) -> Result<(), DbError>;

    async fn get_provider(&self, id: &LlmProviderId) -> Result<Option<LlmProviderRecord>, DbError>;

    async fn list_providers(&self) -> Result<Vec<LlmProviderRecord>, DbError>;

    async fn get_default_provider(&self) -> Result<Option<LlmProviderRecord>, DbError>;
}
