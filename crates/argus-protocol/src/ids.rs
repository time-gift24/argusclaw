use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Session ID - INTEGER auto-increment from database
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub i64);

impl SessionId {
    pub fn new(id: i64) -> Self {
        Self(id)
    }

    pub fn inner(&self) -> i64 {
        self.0
    }
}

/// Thread ID - UUID wrapper for thread identification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ThreadId(pub Uuid);

impl ThreadId {
    /// Create a new unique ThreadId.
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parse a ThreadId from a string representation.
    pub fn parse(s: &str) -> Result<Self, uuid::Error> {
        Ok(Self(Uuid::parse_str(s)?))
    }

    /// Parse a ThreadId from a string, returning a default value on error.
    pub fn parse_or_default(s: &str) -> Self {
        Self::parse(s).unwrap_or_default()
    }

    /// Get the inner UUID value.
    pub fn inner(&self) -> &Uuid {
        &self.0
    }
}

impl Default for ThreadId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ThreadId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Agent ID - INTEGER auto-increment from database
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AgentId(pub i64);

impl AgentId {
    pub fn new(id: i64) -> Self {
        Self(id)
    }

    pub fn inner(&self) -> i64 {
        self.0
    }

    /// Consumes this ID and returns the inner value.
    pub fn into_inner(self) -> i64 {
        self.0
    }
}

impl std::fmt::Display for AgentId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Provider ID - INTEGER auto-increment from database
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ProviderId(pub i64);

impl ProviderId {
    pub fn new(id: i64) -> Self {
        Self(id)
    }

    pub fn inner(&self) -> i64 {
        self.0
    }

    /// Consumes this ID and returns the inner value.
    pub fn into_inner(self) -> i64 {
        self.0
    }
}

impl std::fmt::Display for ProviderId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}
