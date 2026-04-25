use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};

use argus_protocol::{ArgusError, McpRuntimeHeaderOverrides, Result, ThreadId};
use argus_repository::types::AgentRunId;

#[derive(Clone, Default)]
pub struct AgentRunContextRegistry {
    inner: Arc<RwLock<AgentRunContextState>>,
}

#[derive(Default)]
struct AgentRunContextState {
    runs: HashMap<AgentRunId, AgentRunContext>,
    thread_to_run: HashMap<ThreadId, AgentRunId>,
}

#[derive(Clone)]
struct AgentRunContext {
    headers: McpRuntimeHeaderOverrides,
    threads: HashSet<ThreadId>,
}

impl AgentRunContextRegistry {
    pub fn register_run_thread(
        &self,
        run_id: AgentRunId,
        thread_id: ThreadId,
        headers: McpRuntimeHeaderOverrides,
    ) {
        let mut state = self
            .inner
            .write()
            .expect("agent run context registry lock poisoned");
        state.thread_to_run.insert(thread_id, run_id);
        state.runs.insert(
            run_id,
            AgentRunContext {
                headers,
                threads: HashSet::from([thread_id]),
            },
        );
    }

    pub fn inherit_thread(
        &self,
        parent_thread_id: ThreadId,
        child_thread_id: ThreadId,
    ) -> Result<()> {
        let mut state = self
            .inner
            .write()
            .expect("agent run context registry lock poisoned");
        let run_id = state
            .thread_to_run
            .get(&parent_thread_id)
            .copied()
            .ok_or_else(|| ArgusError::ThreadNotFound(parent_thread_id.to_string()))?;
        let context = state
            .runs
            .get_mut(&run_id)
            .ok_or_else(|| ArgusError::ThreadNotFound(parent_thread_id.to_string()))?;
        context.threads.insert(child_thread_id);
        state.thread_to_run.insert(child_thread_id, run_id);
        Ok(())
    }

    pub fn headers_for_thread(&self, thread_id: ThreadId) -> McpRuntimeHeaderOverrides {
        let state = self
            .inner
            .read()
            .expect("agent run context registry lock poisoned");
        state
            .thread_to_run
            .get(&thread_id)
            .and_then(|run_id| state.runs.get(run_id))
            .map(|context| context.headers.clone())
            .unwrap_or_default()
    }

    pub fn remove_run(&self, run_id: AgentRunId) {
        let mut state = self
            .inner
            .write()
            .expect("agent run context registry lock poisoned");
        let Some(context) = state.runs.remove(&run_id) else {
            return;
        };
        for thread_id in context.threads {
            state.thread_to_run.remove(&thread_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use argus_protocol::McpRuntimeHeaders;

    use super::*;

    fn runtime_headers() -> McpRuntimeHeaderOverrides {
        let mut headers = McpRuntimeHeaders::empty();
        headers.insert("Authorization", "Bearer runtime");
        let mut overrides = McpRuntimeHeaderOverrides::empty();
        overrides.insert(12, headers);
        overrides
    }

    #[tokio::test]
    async fn registry_inherits_run_headers_from_parent_to_child_thread() {
        let registry = AgentRunContextRegistry::default();
        let run_id = AgentRunId::new();
        let parent = ThreadId::new();
        let child = ThreadId::new();
        let headers = runtime_headers();

        registry.register_run_thread(run_id, parent, headers.clone());
        registry
            .inherit_thread(parent, child)
            .expect("child thread should inherit run context");

        assert_eq!(registry.headers_for_thread(parent), headers);
        assert_eq!(registry.headers_for_thread(child), headers);
    }

    #[tokio::test]
    async fn registry_cleanup_removes_all_thread_indexes_for_run() {
        let registry = AgentRunContextRegistry::default();
        let run_id = AgentRunId::new();
        let parent = ThreadId::new();
        let child = ThreadId::new();

        registry.register_run_thread(run_id, parent, runtime_headers());
        registry
            .inherit_thread(parent, child)
            .expect("child thread should inherit run context");
        registry.remove_run(run_id);

        assert!(registry.headers_for_thread(parent).is_empty());
        assert!(registry.headers_for_thread(child).is_empty());
    }
}
