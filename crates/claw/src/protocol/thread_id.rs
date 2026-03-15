//! Thread identifier type.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Unique identifier for a Thread.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_id_new_creates_unique_ids() {
        let id1 = ThreadId::new();
        let id2 = ThreadId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn thread_id_default_creates_new_id() {
        let id = ThreadId::default();
        assert!(!id.0.is_nil());
    }

    #[test]
    fn thread_id_display() {
        let id = ThreadId::new();
        let display = format!("{}", id);
        assert!(!display.is_empty());
        assert_eq!(display.len(), 36); // UUID format: 8-4-4-4-12
    }

    #[test]
    fn thread_id_serde_roundtrip() {
        let id = ThreadId::new();
        let json = serde_json::to_string(&id).unwrap();
        let back: ThreadId = serde_json::from_str(&json).unwrap();
        assert_eq!(id, back);
    }
}
