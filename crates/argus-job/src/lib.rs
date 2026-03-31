//! argus-job: Job execution module for argusclaw.
//!
//! Provides JobManager for dispatching background jobs from agents via the
//! unified pipe, and job lifecycle management.

pub mod error;
pub mod job_manager;
pub mod thread_pool;
pub mod types;

pub use error::JobError;
pub use job_manager::{JobLookup, JobManager};
pub use thread_pool::ThreadPool;
pub use types::ThreadPoolJobRequest;

#[cfg(test)]
#[tokio::test]
async fn enqueue_job_creates_binding_and_updates_metrics() {
    thread_pool::assert_enqueue_job_creates_binding_and_updates_metrics().await;
}
