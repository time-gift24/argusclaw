use super::*;

impl JobManager {
    /// Get the currently bound execution thread for a job, if any.
    pub fn thread_binding(&self, job_id: &str) -> Option<ThreadId> {
        self.job_runtime_store
            .lock()
            .expect("job runtime mutex poisoned")
            .job_bindings
            .get(job_id)
            .copied()
    }

    pub fn parent_job_thread_id(&self, child_thread_id: &ThreadId) -> Option<ThreadId> {
        self.job_runtime_store
            .lock()
            .expect("job runtime mutex poisoned")
            .parent_thread_by_child
            .get(child_thread_id)
            .copied()
    }

    pub async fn recover_job_execution_thread_id(
        &self,
        job_id: &str,
    ) -> Result<Option<ThreadId>, JobError> {
        if let Some(thread_id) = self.thread_binding(job_id) {
            return Ok(Some(thread_id));
        }

        if self.thread_repository.is_none() {
            return Ok(None);
        }
        let Some(job_repository) = self.job_repository.as_ref() else {
            return Ok(None);
        };
        let Some(job_record) = job_repository
            .get(&JobId::new(job_id))
            .await
            .map_err(|err| {
                JobError::ExecutionFailed(format!("failed to load job record: {err}"))
            })?
        else {
            return Ok(None);
        };
        let Some(thread_id) = job_record.thread_id else {
            return Ok(None);
        };
        self.cache_job_binding(job_id.to_string(), thread_id);

        if let Some(metadata) = self.recover_job_thread_metadata(thread_id).await? {
            self.sync_job_runtime_metadata(
                metadata.thread_id,
                metadata.job_id,
                metadata.parent_thread_id,
            );
        }

        Ok(Some(thread_id))
    }

    pub async fn recover_parent_job_thread_id(
        &self,
        child_thread_id: &ThreadId,
    ) -> Result<Option<ThreadId>, JobError> {
        if let Some(parent_thread_id) = self.parent_job_thread_id(child_thread_id) {
            return Ok(Some(parent_thread_id));
        }

        Ok(self
            .recover_job_thread_metadata(*child_thread_id)
            .await?
            .and_then(|metadata| metadata.parent_thread_id))
    }

    pub(super) async fn recover_child_jobs_for_thread_inner(
        &self,
        parent_thread_id: ThreadId,
    ) -> Result<Vec<RecoveredChildJob>, JobError> {
        if let Some(children) = self
            .job_runtime_store
            .lock()
            .expect("job runtime mutex poisoned")
            .child_jobs_by_parent
            .get(&parent_thread_id)
            .cloned()
        {
            return Ok(children);
        }

        let parent_base_dir = self.trace_base_dir_for_thread(parent_thread_id).await?;
        let metadata = list_direct_child_threads(&parent_base_dir, parent_thread_id)
            .await
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        let mut children = Vec::with_capacity(metadata.len());
        for child_metadata in metadata {
            let job_id = child_metadata.job_id.clone().ok_or_else(|| {
                JobError::ExecutionFailed(format!(
                    "job thread {} is missing persisted job_id metadata",
                    child_metadata.thread_id
                ))
            })?;
            self.sync_job_runtime_metadata(
                child_metadata.thread_id,
                child_metadata.job_id.clone(),
                child_metadata.parent_thread_id,
            );
            children.push(RecoveredChildJob {
                thread_id: child_metadata.thread_id,
                job_id,
            });
        }
        {
            let mut store = self
                .job_runtime_store
                .lock()
                .expect("job runtime mutex poisoned");
            for child in &children {
                store
                    .job_bindings
                    .insert(child.job_id.clone(), child.thread_id);
                store
                    .parent_thread_by_child
                    .insert(child.thread_id, parent_thread_id);
            }
            store
                .child_jobs_by_parent
                .insert(parent_thread_id, children.clone());
        }
        Ok(children)
    }

    pub(super) async fn recover_job_thread_metadata(
        &self,
        thread_id: ThreadId,
    ) -> Result<Option<ThreadTraceMetadata>, JobError> {
        let base_dir = match find_job_thread_base_dir(&self.trace_dir, thread_id).await {
            Ok(base_dir) => base_dir,
            Err(argus_agent::error::TurnLogError::ThreadMetadataNotFound(_)) => {
                return Ok(None);
            }
            Err(error) => {
                return Err(JobError::ExecutionFailed(format!(
                    "failed to locate job trace metadata: {error}"
                )));
            }
        };
        let metadata = recover_and_validate_metadata(&base_dir, thread_id, ThreadTraceKind::Job)
            .await
            .map_err(|err| JobError::ExecutionFailed(err.to_string()))?;
        self.sync_job_runtime_metadata(
            metadata.thread_id,
            metadata.job_id.clone(),
            metadata.parent_thread_id,
        );
        Ok(Some(metadata))
    }

    pub(super) async fn trace_base_dir_for_thread(
        &self,
        thread_id: ThreadId,
    ) -> Result<PathBuf, JobError> {
        if let Some(thread) = self.thread_pool.loaded_thread(&thread_id) {
            return thread.trace_base_dir().ok_or_else(|| {
                JobError::ExecutionFailed(format!(
                    "thread {} does not expose a trace directory",
                    thread_id
                ))
            });
        }

        if let Some(thread_repository) = self.thread_repository()
            && let Some(thread_record) =
                thread_repository
                    .get_thread(&thread_id)
                    .await
                    .map_err(|err| {
                        JobError::ExecutionFailed(format!("failed to load thread record: {err}"))
                    })?
            && let Some(session_id) = thread_record.session_id
        {
            return Ok(argus_agent::thread_trace_store::chat_thread_base_dir(
                &self.trace_dir,
                session_id,
                thread_id,
            ));
        }

        find_job_thread_base_dir(&self.trace_dir, thread_id)
            .await
            .map_err(|_| {
                JobError::ExecutionFailed(format!("thread {} trace directory not found", thread_id))
            })
    }

    pub(super) fn cache_job_binding(&self, job_id: String, thread_id: ThreadId) {
        self.job_runtime_store
            .lock()
            .expect("job runtime mutex poisoned")
            .job_bindings
            .insert(job_id, thread_id);
    }

    fn cache_parent_job_thread(
        &self,
        child_thread_id: ThreadId,
        parent_thread_id: ThreadId,
        job_id: Option<String>,
    ) {
        let mut store = self
            .job_runtime_store
            .lock()
            .expect("job runtime mutex poisoned");
        store
            .parent_thread_by_child
            .insert(child_thread_id, parent_thread_id);
        let children = store
            .child_jobs_by_parent
            .entry(parent_thread_id)
            .or_default();
        if let Some(existing) = children
            .iter_mut()
            .find(|child| child.thread_id == child_thread_id)
        {
            if let Some(job_id) = job_id {
                existing.job_id = job_id;
            }
            return;
        }
        children.push(RecoveredChildJob {
            thread_id: child_thread_id,
            job_id: job_id.unwrap_or_default(),
        });
    }

    pub(super) fn sync_job_runtime_metadata(
        &self,
        thread_id: ThreadId,
        job_id: Option<String>,
        parent_thread_id: Option<ThreadId>,
    ) {
        if let Some(job_id) = job_id.clone() {
            self.cache_job_binding(job_id.clone(), thread_id);
            if !self
                .job_runtime_store
                .lock()
                .expect("job runtime mutex poisoned")
                .job_runtimes
                .contains_key(&thread_id)
            {
                self.upsert_job_runtime_summary(
                    thread_id,
                    job_id,
                    ThreadRuntimeStatus::Inactive,
                    0,
                    None,
                    true,
                    None,
                );
            }
        }
        if let Some(parent_thread_id) = parent_thread_id {
            self.cache_parent_job_thread(thread_id, parent_thread_id, job_id);
        }
    }
}
