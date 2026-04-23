use super::*;

#[derive(Debug, Default)]
pub(super) struct JobRuntimeStore {
    pub(super) job_bindings: HashMap<String, ThreadId>,
    pub(super) parent_thread_by_child: HashMap<ThreadId, ThreadId>,
    pub(super) child_jobs_by_parent: HashMap<ThreadId, Vec<RecoveredChildJob>>,
    pub(super) delivered_job_results: HashMap<ThreadId, Vec<MailboxMessage>>,
    pub(super) job_runtimes: HashMap<ThreadId, JobRuntimeSummary>,
    pub(super) peak_estimated_memory_bytes: u64,
}

impl JobManager {
    /// Collect the authoritative job-runtime state.
    pub fn job_runtime_state(&self) -> JobRuntimeState {
        let runtimes = self.current_job_runtime_summaries();
        let snapshot = self.collect_job_runtime_snapshot(&runtimes);
        JobRuntimeState { snapshot, runtimes }
    }

    pub fn job_runtime_summary(&self, thread_id: &ThreadId) -> Option<JobRuntimeSummary> {
        self.job_runtime_store
            .lock()
            .expect("job runtime mutex poisoned")
            .job_runtimes
            .get(thread_id)
            .cloned()
    }

    fn current_job_runtime_summaries(&self) -> Vec<JobRuntimeSummary> {
        self.job_runtime_store
            .lock()
            .expect("job runtime mutex poisoned")
            .job_runtimes
            .values()
            .cloned()
            .collect()
    }

    fn collect_job_runtime_snapshot(&self, runtimes: &[JobRuntimeSummary]) -> JobRuntimeSnapshot {
        let peak_estimated_memory_bytes = self
            .job_runtime_store
            .lock()
            .expect("job runtime mutex poisoned")
            .peak_estimated_memory_bytes;
        Self::build_job_runtime_snapshot(
            self.thread_pool.collect_metrics().max_threads,
            peak_estimated_memory_bytes,
            runtimes,
        )
    }

    pub(super) fn build_job_runtime_snapshot(
        max_threads: u32,
        peak_estimated_memory_bytes: u64,
        runtimes: &[JobRuntimeSummary],
    ) -> JobRuntimeSnapshot {
        let active_threads = runtimes
            .iter()
            .filter(|runtime| {
                matches!(
                    runtime.status,
                    ThreadRuntimeStatus::Loading
                        | ThreadRuntimeStatus::Queued
                        | ThreadRuntimeStatus::Running
                        | ThreadRuntimeStatus::Cooling
                )
            })
            .count() as u32;
        let queued_threads = runtimes
            .iter()
            .filter(|runtime| runtime.status == ThreadRuntimeStatus::Queued)
            .count() as u32;
        let running_threads = runtimes
            .iter()
            .filter(|runtime| runtime.status == ThreadRuntimeStatus::Running)
            .count() as u32;
        let cooling_threads = runtimes
            .iter()
            .filter(|runtime| runtime.status == ThreadRuntimeStatus::Cooling)
            .count() as u32;
        let evicted_threads = runtimes
            .iter()
            .filter(|runtime| runtime.status == ThreadRuntimeStatus::Evicted)
            .count() as u64;
        let estimated_memory_bytes = runtimes
            .iter()
            .filter(|runtime| {
                matches!(
                    runtime.status,
                    ThreadRuntimeStatus::Loading
                        | ThreadRuntimeStatus::Queued
                        | ThreadRuntimeStatus::Running
                        | ThreadRuntimeStatus::Cooling
                )
            })
            .map(|runtime| runtime.estimated_memory_bytes)
            .sum();
        let resident_thread_count = runtimes
            .iter()
            .filter(|runtime| {
                matches!(
                    runtime.status,
                    ThreadRuntimeStatus::Loading
                        | ThreadRuntimeStatus::Queued
                        | ThreadRuntimeStatus::Running
                        | ThreadRuntimeStatus::Cooling
                )
            })
            .count() as u32;
        let avg_thread_memory_bytes = if resident_thread_count == 0 {
            0
        } else {
            estimated_memory_bytes / u64::from(resident_thread_count)
        };

        JobRuntimeSnapshot {
            max_threads,
            active_threads,
            queued_threads,
            running_threads,
            cooling_threads,
            evicted_threads,
            estimated_memory_bytes,
            peak_estimated_memory_bytes,
            process_memory_bytes: None,
            peak_process_memory_bytes: None,
            resident_thread_count,
            avg_thread_memory_bytes,
            captured_at: Utc::now().to_rfc3339(),
        }
    }

    fn refresh_job_runtime_peaks(store: &mut JobRuntimeStore) {
        let current_estimated: u64 = store
            .job_runtimes
            .values()
            .filter(|runtime| {
                matches!(
                    runtime.status,
                    ThreadRuntimeStatus::Loading
                        | ThreadRuntimeStatus::Queued
                        | ThreadRuntimeStatus::Running
                        | ThreadRuntimeStatus::Cooling
                )
            })
            .map(|runtime| runtime.estimated_memory_bytes)
            .sum();
        if current_estimated > store.peak_estimated_memory_bytes {
            store.peak_estimated_memory_bytes = current_estimated;
        }
    }

    fn merge_job_runtime_summary(
        store: &mut JobRuntimeStore,
        runtime: JobRuntimeSummary,
    ) -> JobRuntimeSummary {
        store
            .job_runtimes
            .insert(runtime.thread_id, runtime.clone());
        Self::refresh_job_runtime_peaks(store);
        runtime
    }

    fn update_job_runtime_summary_for_thread(
        store: &mut JobRuntimeStore,
        thread_id: ThreadId,
        status: ThreadRuntimeStatus,
        estimated_memory_bytes: u64,
        last_active_at: Option<String>,
        recoverable: bool,
        last_reason: Option<ThreadPoolEventReason>,
    ) -> Option<JobRuntimeSummary> {
        let runtime = store.job_runtimes.get_mut(&thread_id)?;
        runtime.status = status;
        runtime.estimated_memory_bytes = estimated_memory_bytes;
        runtime.last_active_at = last_active_at;
        runtime.recoverable = recoverable;
        runtime.last_reason = last_reason;
        let runtime = runtime.clone();
        Self::refresh_job_runtime_peaks(store);
        Some(runtime)
    }

    pub(super) fn upsert_job_runtime_summary(
        &self,
        thread_id: ThreadId,
        job_id: String,
        status: ThreadRuntimeStatus,
        estimated_memory_bytes: u64,
        last_active_at: Option<String>,
        recoverable: bool,
        last_reason: Option<ThreadPoolEventReason>,
    ) -> JobRuntimeSummary {
        let mut store = self
            .job_runtime_store
            .lock()
            .expect("job runtime mutex poisoned");
        Self::merge_job_runtime_summary(
            &mut store,
            JobRuntimeSummary {
                thread_id,
                job_id,
                status,
                estimated_memory_bytes,
                last_active_at,
                recoverable,
                last_reason,
            },
        )
    }

    pub(super) fn install_runtime_lifecycle_bridge(&self) {
        let thread_pool = Arc::downgrade(&self.thread_pool);
        let job_runtime_store = Arc::downgrade(&self.job_runtime_store);
        self.thread_pool
            .add_runtime_lifecycle_observer(Arc::new(move |change| {
                Self::handle_runtime_lifecycle_change(&thread_pool, &job_runtime_store, change);
            }));
    }

    fn handle_runtime_lifecycle_change(
        thread_pool: &Weak<ThreadPool>,
        job_runtime_store: &Weak<StdMutex<JobRuntimeStore>>,
        change: RuntimeLifecycleChange,
    ) {
        let Some(thread_pool) = thread_pool.upgrade() else {
            return;
        };
        let Some(job_runtime_store) = job_runtime_store.upgrade() else {
            return;
        };

        let runtime = match change {
            RuntimeLifecycleChange::Evicted(runtime) => runtime,
            RuntimeLifecycleChange::Cooling(_) => return,
        };
        let (parent_thread_id, runtime) = {
            let mut store = job_runtime_store
                .lock()
                .expect("job runtime mutex poisoned");
            let Some(runtime) = Self::update_job_runtime_summary_for_thread(
                &mut store,
                runtime.thread_id,
                runtime.status,
                runtime.estimated_memory_bytes,
                runtime.last_active_at,
                runtime.recoverable,
                runtime.last_reason.clone(),
            ) else {
                return;
            };
            let Some(parent_thread_id) = store
                .parent_thread_by_child
                .get(&runtime.thread_id)
                .copied()
            else {
                return;
            };
            (parent_thread_id, runtime)
        };

        if !thread_pool.emit_observer_event(
            &parent_thread_id,
            ThreadEvent::JobRuntimeUpdated {
                runtime: runtime.clone(),
            },
        ) {
            return;
        }
        let _ = thread_pool.emit_observer_event(
            &parent_thread_id,
            ThreadEvent::JobRuntimeEvicted {
                thread_id: runtime.thread_id,
                job_id: runtime.job_id.clone(),
                reason: runtime
                    .last_reason
                    .clone()
                    .unwrap_or(ThreadPoolEventReason::MemoryPressure),
            },
        );
        let snapshot = {
            let store = job_runtime_store
                .lock()
                .expect("job runtime mutex poisoned");
            let runtimes: Vec<_> = store.job_runtimes.values().cloned().collect();
            Self::build_job_runtime_snapshot(
                thread_pool.collect_metrics().max_threads,
                store.peak_estimated_memory_bytes,
                &runtimes,
            )
        };
        let _ = thread_pool.emit_observer_event(
            &parent_thread_id,
            ThreadEvent::JobRuntimeMetricsUpdated { snapshot },
        );
    }

    pub(super) fn emit_job_runtime_updated(
        pipe_tx: &broadcast::Sender<ThreadEvent>,
        runtime: &JobRuntimeSummary,
    ) {
        let _ = pipe_tx.send(ThreadEvent::JobRuntimeUpdated {
            runtime: runtime.clone(),
        });
    }

    pub(super) fn emit_job_runtime_metrics(&self, pipe_tx: &broadcast::Sender<ThreadEvent>) {
        let _ = pipe_tx.send(ThreadEvent::JobRuntimeMetricsUpdated {
            snapshot: self.job_runtime_state().snapshot,
        });
    }
}
