//! argus-job: Job execution module for argusclaw.
//!
//! Provides JobManager for dispatching background jobs from agents via the
//! unified pipe, and job lifecycle management.

pub mod dispatch_tool;
pub mod error;
pub mod get_job_result_tool;
pub mod job_manager;
pub mod list_subagents_tool;
pub mod thread_pool;
pub mod types;

pub use dispatch_tool::DispatchJobTool;
pub use error::JobError;
pub use get_job_result_tool::GetJobResultTool;
pub use job_manager::{JobLookup, JobManager};
pub use list_subagents_tool::ListSubagentsTool;
pub use thread_pool::ThreadPool;
pub use types::{GetJobResultArgs, JobDispatchArgs, JobResult};

#[cfg(test)]
#[tokio::test]
async fn enqueue_job_creates_binding_and_updates_metrics() {
    thread_pool::assert_enqueue_job_creates_binding_and_updates_metrics().await;
}
