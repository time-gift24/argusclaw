//! Agent management module.

pub mod turn;

mod types;

pub use types::{AgentId, AgentRecord, AgentRepository, AgentSummary};

use std::sync::Arc;

use dashmap::DashMap;

use crate::db::DbError;

/// Manages custom agents with in-memory caching.
#[derive(Clone)]
pub struct AgentManager {
    repository: Arc<dyn AgentRepository>,
    cache: DashMap<AgentId, AgentRecord>,
}

impl AgentManager {
    #[must_use]
    pub fn new(repository: Arc<dyn AgentRepository>) -> Self {
        Self {
            repository,
            cache: DashMap::new(),
        }
    }

    /// Create or update an agent.
    pub async fn upsert(&self, record: AgentRecord) -> Result<(), DbError> {
        self.repository.upsert(&record).await?;
        self.cache.insert(record.id.clone(), record);
        Ok(())
    }

    /// Get an agent by ID with read-through cache.
    pub async fn get(&self, id: &AgentId) -> Result<Option<AgentRecord>, DbError> {
        if let Some(cached) = self.cache.get(id) {
            return Ok(Some(cached.clone()));
        }

        if let Some(record) = self.repository.get(id).await? {
            self.cache.insert(id.clone(), record.clone());
            return Ok(Some(record));
        }

        Ok(None)
    }

    /// List all agents (summaries only).
    pub async fn list(&self) -> Result<Vec<AgentSummary>, DbError> {
        self.repository.list().await
    }

    /// Delete an agent.
    pub async fn delete(&self, id: &AgentId) -> Result<bool, DbError> {
        let deleted = self.repository.delete(id).await?;
        if deleted {
            self.cache.remove(id);
        }
        Ok(deleted)
    }
}
