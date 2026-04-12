//! argus-job: Job execution module for argusclaw.
//!
//! Provides JobManager for dispatching background jobs from agents via the
//! unified pipe, and job lifecycle management.

pub mod error;
pub mod job_manager;
pub mod job_runtime_supervisor;
pub mod types;

pub use error::JobError;
pub use job_manager::{JobLookup, JobManager};
pub use job_runtime_supervisor::{JobRuntimePersistence, JobRuntimeSupervisor, RecoveredChildJob};
pub use types::JobRuntimeRequest;

#[cfg(test)]
#[tokio::test]
async fn enqueue_job_creates_binding_and_updates_metrics() {
    job_runtime_supervisor::assert_enqueue_job_creates_binding_and_updates_metrics().await;
}
