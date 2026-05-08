use super::*;

#[derive(Debug, Clone)]
pub(super) enum TrackedJobState {
    Pending,
    Cancelling,
    Completed(ThreadJobResult),
    Consumed(ThreadJobResult),
}

#[derive(Debug, Clone)]
pub(super) struct TrackedJob {
    pub(super) thread_id: ThreadId,
    pub(super) state: TrackedJobState,
    /// Cancellation handle for Pending jobs; None once Completed/Consumed.
    pub(super) cancellation: Option<TurnCancellation>,
    pub(super) generation: u64,
}

#[derive(Debug, Default)]
pub(super) struct TrackedJobsStore {
    pub(super) jobs: HashMap<String, TrackedJob>,
    pub(super) terminal_order: VecDeque<(String, u64)>,
    pub(super) next_generation: u64,
}

impl JobManager {
    /// Stop a running background job by signalling cancellation.
    ///
    /// Returns `JobNotFound` if the job was never dispatched,
    /// or `JobNotRunning` if it already completed.
    pub fn stop_job(&self, job_id: &str) -> Result<(), JobError> {
        let cancellation = {
            let mut tracked_jobs = self
                .tracked_jobs
                .lock()
                .expect("job tracking mutex poisoned");

            let tracked_job = tracked_jobs
                .jobs
                .get_mut(job_id)
                .ok_or_else(|| JobError::JobNotFound(job_id.to_string()))?;

            match &tracked_job.state {
                TrackedJobState::Pending => {}
                TrackedJobState::Cancelling
                | TrackedJobState::Completed(_)
                | TrackedJobState::Consumed(_) => {
                    return Err(JobError::JobNotRunning(job_id.to_string()));
                }
            }

            if tracked_job.cancellation.is_none() || !self.is_job_runtime_active(job_id) {
                return Err(JobError::JobNotRunning(job_id.to_string()));
            }

            let cancellation = tracked_job
                .cancellation
                .take()
                .ok_or_else(|| JobError::JobNotRunning(job_id.to_string()))?;
            tracked_job.state = TrackedJobState::Cancelling;
            cancellation
        };

        cancellation.cancel();

        Ok(())
    }

    /// Record that a job was dispatched for a thread.
    pub fn record_dispatched_job(&self, thread_id: ThreadId, job_id: String) {
        Self::record_dispatched_job_in_store(
            &self.tracked_jobs,
            thread_id,
            job_id,
            TurnCancellation::new(),
        );
    }

    /// Record the completed result for a job.
    pub fn record_completed_job_result(&self, thread_id: ThreadId, result: ThreadJobResult) {
        Self::record_completed_job_result_in_store(&self.tracked_jobs, thread_id, result);
    }

    /// Get the current tracked status for a job scoped to its originating thread.
    pub fn get_job_result_status(
        &self,
        thread_id: ThreadId,
        job_id: &str,
        consume: bool,
    ) -> JobLookup {
        let mut tracked_jobs = self
            .tracked_jobs
            .lock()
            .expect("job tracking mutex poisoned");
        Self::lookup_job_in_store(&mut tracked_jobs, thread_id, job_id, consume)
    }

    /// Get the current status for a job, recovering persisted state when caches are cold.
    pub async fn get_job_result_status_persisted(
        &self,
        thread_id: ThreadId,
        job_id: &str,
        consume: bool,
    ) -> Result<JobLookup, JobError> {
        {
            let mut tracked_jobs = self
                .tracked_jobs
                .lock()
                .expect("job tracking mutex poisoned");
            let lookup = Self::lookup_job_in_store(&mut tracked_jobs, thread_id, job_id, consume);
            if !matches!(lookup, JobLookup::NotFound) {
                return Ok(lookup);
            }
        }

        let Some(job_repository) = &self.job_repository else {
            return Ok(JobLookup::NotFound);
        };
        let Some(job_record) = job_repository
            .get(&JobId::new(job_id))
            .await
            .map_err(|err| {
                JobError::ExecutionFailed(format!("failed to load job record: {err}"))
            })?
        else {
            return Ok(JobLookup::NotFound);
        };
        let Some(execution_thread_id) = job_record.thread_id else {
            return Ok(JobLookup::NotFound);
        };
        let Some(metadata) = self
            .recover_job_thread_metadata(execution_thread_id)
            .await?
        else {
            return Ok(JobLookup::NotFound);
        };
        self.sync_job_runtime_metadata(
            metadata.thread_id,
            metadata.job_id.clone(),
            metadata.parent_thread_id,
        );
        if metadata.parent_thread_id != Some(thread_id)
            || metadata.job_id.as_deref() != Some(job_id)
        {
            return Ok(JobLookup::NotFound);
        }

        match job_record.status {
            JobStatus::Pending | JobStatus::Queued | JobStatus::Running | JobStatus::Paused => {
                Ok(JobLookup::Pending)
            }
            JobStatus::Succeeded | JobStatus::Failed | JobStatus::Cancelled => {
                let Some(result) = job_record.result else {
                    return Ok(JobLookup::NotFound);
                };
                let persisted = ThreadJobResult {
                    job_id: job_id.to_string(),
                    success: result.success,
                    cancelled: matches!(job_record.status, JobStatus::Cancelled),
                    message: result.message,
                    token_usage: result.token_usage,
                    agent_id: AgentId::new(result.agent_id.inner()),
                    agent_display_name: result.agent_display_name,
                    agent_description: result.agent_description,
                };
                Self::record_completed_job_result_in_store(
                    &self.tracked_jobs,
                    thread_id,
                    persisted.clone(),
                );
                let mut tracked_jobs = self
                    .tracked_jobs
                    .lock()
                    .expect("job tracking mutex poisoned");
                Ok(Self::lookup_job_in_store(
                    &mut tracked_jobs,
                    thread_id,
                    job_id,
                    consume,
                ))
            }
        }
    }

    pub fn is_job_pending(&self, job_id: &str) -> bool {
        let tracked_jobs = self
            .tracked_jobs
            .lock()
            .expect("job tracking mutex poisoned");

        tracked_jobs
            .jobs
            .get(job_id)
            .is_some_and(|tracked_job| matches!(tracked_job.state, TrackedJobState::Pending))
    }

    pub async fn is_job_pending_persisted(&self, job_id: &str) -> Result<bool, JobError> {
        {
            let tracked_jobs = self
                .tracked_jobs
                .lock()
                .expect("job tracking mutex poisoned");
            if let Some(tracked_job) = tracked_jobs.jobs.get(job_id) {
                return Ok(matches!(
                    tracked_job.state,
                    TrackedJobState::Pending | TrackedJobState::Cancelling
                ));
            }
        }

        let Some(job_repository) = &self.job_repository else {
            return Ok(false);
        };
        let Some(job_record) = job_repository
            .get(&JobId::new(job_id))
            .await
            .map_err(|err| {
                JobError::ExecutionFailed(format!("failed to load job record: {err}"))
            })?
        else {
            return Ok(false);
        };

        Ok(matches!(
            job_record.status,
            JobStatus::Pending | JobStatus::Queued | JobStatus::Running
        ))
    }

    pub(super) fn record_dispatched_job_in_store(
        tracked_jobs: &Arc<StdMutex<TrackedJobsStore>>,
        thread_id: ThreadId,
        job_id: String,
        cancellation: TurnCancellation,
    ) {
        let mut tracked_jobs = tracked_jobs.lock().expect("job tracking mutex poisoned");
        let generation = tracked_jobs.next_generation;
        tracked_jobs.next_generation = tracked_jobs.next_generation.saturating_add(1);
        tracked_jobs.jobs.insert(
            job_id,
            TrackedJob {
                thread_id,
                state: TrackedJobState::Pending,
                cancellation: Some(cancellation),
                generation,
            },
        );
    }

    pub(super) fn record_completed_job_result_in_store(
        tracked_jobs: &Arc<StdMutex<TrackedJobsStore>>,
        thread_id: ThreadId,
        result: ThreadJobResult,
    ) {
        let mut tracked_jobs = tracked_jobs.lock().expect("job tracking mutex poisoned");
        let generation = tracked_jobs.next_generation;
        tracked_jobs.next_generation = tracked_jobs.next_generation.saturating_add(1);
        let job_id = result.job_id.clone();
        tracked_jobs.jobs.insert(
            job_id.clone(),
            TrackedJob {
                thread_id,
                state: TrackedJobState::Completed(result),
                cancellation: None,
                generation,
            },
        );
        tracked_jobs.terminal_order.push_back((job_id, generation));
        Self::prune_terminal_jobs(&mut tracked_jobs);
    }

    fn prune_terminal_jobs(tracked_jobs: &mut TrackedJobsStore) {
        while tracked_jobs.terminal_order.len() > Self::TERMINAL_JOB_RETENTION_LIMIT {
            let Some((job_id, generation)) = tracked_jobs.terminal_order.pop_front() else {
                break;
            };
            let should_remove = tracked_jobs.jobs.get(&job_id).is_some_and(|tracked_job| {
                tracked_job.generation == generation
                    && matches!(
                        tracked_job.state,
                        TrackedJobState::Completed(_) | TrackedJobState::Consumed(_)
                    )
            });
            if should_remove {
                tracked_jobs.jobs.remove(&job_id);
            }
        }
    }

    fn lookup_job_in_store(
        tracked_jobs: &mut TrackedJobsStore,
        thread_id: ThreadId,
        job_id: &str,
        consume: bool,
    ) -> JobLookup {
        let Some(tracked_job) = tracked_jobs.jobs.get_mut(job_id) else {
            return JobLookup::NotFound;
        };

        if tracked_job.thread_id != thread_id {
            return JobLookup::NotFound;
        }

        match &tracked_job.state {
            TrackedJobState::Pending | TrackedJobState::Cancelling => JobLookup::Pending,
            TrackedJobState::Completed(result) => {
                let result = result.clone();
                if consume {
                    tracked_job.state = TrackedJobState::Consumed(result.clone());
                }
                JobLookup::Completed(result)
            }
            TrackedJobState::Consumed(result) => JobLookup::Consumed(result.clone()),
        }
    }

    fn is_job_runtime_active(&self, job_id: &str) -> bool {
        let Some(thread_id) = self.thread_binding(job_id) else {
            return true;
        };

        self.job_runtime_summary(&thread_id).is_some_and(|runtime| {
            matches!(
                runtime.status,
                ThreadRuntimeStatus::Loading
                    | ThreadRuntimeStatus::Queued
                    | ThreadRuntimeStatus::Running
            )
        })
    }
}
