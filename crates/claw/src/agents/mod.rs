//! Agent management module.

pub mod agent;
pub mod compact;
pub mod thread;
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::RwLock;

    /// In-memory mock repository for testing.
    struct MockAgentRepository {
        agents: RwLock<HashMap<String, AgentRecord>>,
    }

    impl MockAgentRepository {
        fn new() -> Self {
            Self {
                agents: RwLock::new(HashMap::new()),
            }
        }
    }

    #[async_trait::async_trait]
    impl AgentRepository for MockAgentRepository {
        async fn upsert(&self, record: &AgentRecord) -> Result<(), DbError> {
            let mut agents = self.agents.write().unwrap();
            agents.insert(record.id.as_ref().to_string(), record.clone());
            Ok(())
        }

        async fn get(&self, id: &AgentId) -> Result<Option<AgentRecord>, DbError> {
            let agents = self.agents.read().unwrap();
            Ok(agents.get(id.as_ref()).cloned())
        }

        async fn list(&self) -> Result<Vec<AgentSummary>, DbError> {
            let agents = self.agents.read().unwrap();
            Ok(agents.values().map(|r| r.clone().into()).collect())
        }

        async fn delete(&self, id: &AgentId) -> Result<bool, DbError> {
            let mut agents = self.agents.write().unwrap();
            Ok(agents.remove(id.as_ref()).is_some())
        }
    }

    fn create_test_record(id: &str) -> AgentRecord {
        AgentRecord {
            id: AgentId::new(id),
            display_name: format!("Agent {id}"),
            description: "Test agent".to_string(),
            version: "1.0.0".to_string(),
            provider_id: "test-provider".to_string(),
            system_prompt: "You are a test agent.".to_string(),
            tool_names: vec!["tool1".to_string()],
            max_tokens: Some(1000),
            temperature: Some(0.7),
        }
    }

    #[tokio::test]
    async fn upsert_stores_and_retrieves_agent() {
        let repo = Arc::new(MockAgentRepository::new());
        let manager = AgentManager::new(repo);

        let record = create_test_record("agent-1");
        manager.upsert(record.clone()).await.unwrap();

        let retrieved = manager.get(&AgentId::new("agent-1")).await.unwrap();
        assert!(retrieved.is_some());
        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.id, record.id);
        assert_eq!(retrieved.display_name, record.display_name);
        assert_eq!(retrieved.temperature, record.temperature);
    }

    #[tokio::test]
    async fn get_returns_none_for_missing_agent() {
        let repo = Arc::new(MockAgentRepository::new());
        let manager = AgentManager::new(repo);

        let result = manager.get(&AgentId::new("missing")).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn delete_removes_agent() {
        let repo = Arc::new(MockAgentRepository::new());
        let manager = AgentManager::new(repo);

        let record = create_test_record("agent-to-delete");
        manager.upsert(record).await.unwrap();

        let deleted = manager
            .delete(&AgentId::new("agent-to-delete"))
            .await
            .unwrap();
        assert!(deleted);

        let result = manager.get(&AgentId::new("agent-to-delete")).await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn delete_returns_false_for_missing_agent() {
        let repo = Arc::new(MockAgentRepository::new());
        let manager = AgentManager::new(repo);

        let deleted = manager.delete(&AgentId::new("missing")).await.unwrap();
        assert!(!deleted);
    }

    #[tokio::test]
    async fn list_returns_summaries() {
        let repo = Arc::new(MockAgentRepository::new());
        let manager = AgentManager::new(repo);

        manager.upsert(create_test_record("agent-1")).await.unwrap();
        manager.upsert(create_test_record("agent-2")).await.unwrap();

        let summaries = manager.list().await.unwrap();
        assert_eq!(summaries.len(), 2);
    }

    #[tokio::test]
    async fn cache_is_updated_on_upsert() {
        let repo = Arc::new(MockAgentRepository::new());
        let manager = AgentManager::new(repo);

        let mut record = create_test_record("cached-agent");
        manager.upsert(record.clone()).await.unwrap();

        // Update the record
        record.display_name = "Updated Name".to_string();
        manager.upsert(record.clone()).await.unwrap();

        // Should get updated version from cache
        let retrieved = manager
            .get(&AgentId::new("cached-agent"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(retrieved.display_name, "Updated Name");
    }
}
