use std::collections::HashMap;
use tokio::sync::RwLock;

use super::{Workflow, WorkflowError};

pub struct WorkflowManager {
    store: RwLock<HashMap<String, Workflow>>,
}

impl WorkflowManager {
    #[must_use]
    pub fn new() -> Self {
        let mut store = HashMap::new();
        store.insert("demo".to_string(), Workflow::demo());
        Self {
            store: RwLock::new(store),
        }
    }

    pub async fn get(&self, id: &str) -> Result<Workflow, WorkflowError> {
        self.store
            .read()
            .await
            .get(id)
            .cloned()
            .ok_or_else(|| WorkflowError::NotFound(id.to_string()))
    }

    pub async fn save(&self, workflow: &Workflow) -> Result<(), WorkflowError> {
        self.store
            .write()
            .await
            .insert(workflow.id.clone(), workflow.clone());
        Ok(())
    }

    pub async fn list(&self) -> Result<Vec<Workflow>, WorkflowError> {
        Ok(self.store.read().await.values().cloned().collect())
    }
}

impl Default for WorkflowManager {
    fn default() -> Self {
        Self::new()
    }
}
