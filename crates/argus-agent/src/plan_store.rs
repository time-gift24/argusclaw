//! FilePlanStore — file-backed plan storage for per-Thread plan persistence.
//!
//! Stores plan state in `{trace_dir}/{thread_id}/plan.json`, supporting recovery
//! after Thread restarts. Internally holds `Arc<RwLock<Vec<Value>>>` for in-memory
//! reads, with async file writes on every plan update.

use std::env;
use std::path::PathBuf;
use std::sync::{Arc, RwLock};

use serde::Serialize;
use serde_json::Value;

/// File-backed plan store.
///
/// - `store`: shared in-memory plan state
/// - `path`: path to plan.json on disk
#[derive(Clone)]
pub struct FilePlanStore {
    store: Arc<RwLock<Vec<Value>>>,
    path: PathBuf,
}

impl FilePlanStore {
    /// Create a new FilePlanStore.
    ///
    /// The plan.json path is `{trace_dir}/{thread_id}/plan.json`.
    /// If plan.json already exists, loads its content into memory.
    /// IO errors during load are logged and ignored (start with empty plan).
    pub fn new(trace_dir: PathBuf, thread_id: &str) -> Self {
        let dir = trace_dir.join(thread_id);
        let path = dir.join("plan.json");

        // Ensure directory exists
        let _ = std::fs::create_dir_all(&dir);

        // Try to load existing plan from file
        let initial: Vec<Value> = match std::fs::read_to_string(&path) {
            Ok(content) => serde_json::from_str(&content).unwrap_or_default(),
            Err(_) => Vec::new(),
        };

        Self {
            store: Arc::new(RwLock::new(initial)),
            path,
        }
    }

    /// Return the shared in-memory store reference.
    pub fn store(&self) -> Arc<RwLock<Vec<Value>>> {
        self.store.clone()
    }

    /// Update the plan from a list of serializable items: writes to memory and persists.
    ///
    /// Each item is serialized to a JSON Value. File write errors are logged but do not
    /// affect the in-memory state.
    pub fn write_from_items<T: Serialize>(&self, items: Vec<T>) {
        let plan_values: Vec<Value> = items
            .iter()
            .map(|item| serde_json::to_value(item).unwrap())
            .collect();

        // Update memory
        {
            let mut store = self.store.write().unwrap();
            *store = plan_values;
        }

        // Persist to disk asynchronously
        let path = self.path.clone();
        let store = self.store.clone();
        tokio::spawn(async move {
            let data = {
                let store = store.read().unwrap();
                serde_json::to_string_pretty(&*store).unwrap_or_default()
            };
            if let Err(e) = tokio::fs::write(&path, data).await {
                tracing::warn!(path = %path.display(), error = %e, "Failed to persist plan.json");
            }
        });
    }
}

impl Default for FilePlanStore {
    fn default() -> Self {
        Self::new(env::temp_dir(), "default")
    }
}
