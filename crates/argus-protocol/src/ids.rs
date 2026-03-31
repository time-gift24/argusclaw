use schemars::JsonSchema;
use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

/// Session ID - UUIDv7 (time-sortable).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionId(pub Uuid);

impl SessionId {
    /// Create a new SessionId using UUIDv7 (time-sortable).
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    /// Parse a SessionId from a string representation.
    pub fn parse(s: &str) -> Result<Self, uuid::Error> {
        Ok(Self(Uuid::parse_str(s)?))
    }

    /// Get the inner UUID value.
    pub fn inner(&self) -> &Uuid {
        &self.0
    }
}

impl Default for SessionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SessionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Thread ID - UUID wrapper for thread identification.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ThreadId(pub Uuid);

impl ThreadId {
    /// Create a new ThreadId using UUIDv7 (time-sortable).
    pub fn new() -> Self {
        Self(Uuid::now_v7())
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, JsonSchema)]
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

impl<'de> Deserialize<'de> for AgentId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct AgentIdVisitor;

        impl Visitor<'_> for AgentIdVisitor {
            type Value = AgentId;

            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an integer agent id or a string containing an integer")
            }

            fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                Ok(AgentId::new(value))
            }

            fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let value = i64::try_from(value)
                    .map_err(|_| E::custom(format!("agent id {value} exceeds i64 range")))?;
                Ok(AgentId::new(value))
            }

            fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                let value = value
                    .trim()
                    .parse::<i64>()
                    .map_err(|_| E::custom(format!("agent id '{value}' is not a valid integer")))?;
                Ok(AgentId::new(value))
            }

            fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
            where
                E: de::Error,
            {
                self.visit_str(&value)
            }
        }

        deserializer.deserialize_any(AgentIdVisitor)
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

#[cfg(test)]
mod tests {
    use super::AgentId;

    #[test]
    fn agent_id_deserializes_from_integer() {
        let parsed: AgentId = serde_json::from_value(serde_json::json!(7))
            .expect("integer agent id should deserialize");
        assert_eq!(parsed, AgentId::new(7));
    }

    #[test]
    fn agent_id_deserializes_from_numeric_string() {
        let parsed: AgentId = serde_json::from_value(serde_json::json!("7"))
            .expect("numeric string agent id should deserialize");
        assert_eq!(parsed, AgentId::new(7));
    }

    #[test]
    fn agent_id_rejects_non_numeric_string() {
        let error = serde_json::from_value::<AgentId>(serde_json::json!("worker-seven"))
            .expect_err("non-numeric string should fail");
        assert!(
            error.to_string().contains("not a valid integer"),
            "unexpected error: {error}"
        );
    }
}
