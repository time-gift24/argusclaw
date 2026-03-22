use std::fmt;

use serde::{Deserialize, Serialize};

use argus_protocol::SessionId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionIdRecord(pub i64);

impl SessionIdRecord {
    pub fn new(id: i64) -> Self {
        Self(id)
    }
}

impl fmt::Display for SessionIdRecord {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<SessionId> for SessionIdRecord {
    fn from(id: SessionId) -> Self {
        Self(id.inner())
    }
}

impl From<SessionIdRecord> for SessionId {
    fn from(id: SessionIdRecord) -> Self {
        SessionId::new(id.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    pub id: SessionId,
    pub name: String,
    pub created_at: String,
    pub updated_at: String,
}
