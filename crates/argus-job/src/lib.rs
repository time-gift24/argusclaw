//! argus-job: Job execution module for argusclaw.
//!
//! Provides JobManager for dispatching background jobs from agents,
//! SSE-based completion notification, and job lifecycle management.

pub mod dispatch_tool;
pub mod error;
pub mod get_job_result_tool;
pub mod list_subagents_tool;
pub mod job_manager;
pub mod sse_broadcaster;
pub mod types;

pub use dispatch_tool::DispatchJobTool;
pub use error::JobError;
pub use get_job_result_tool::GetJobResultTool;
pub use job_manager::JobManager;
pub use list_subagents_tool::ListSubagentsTool;
pub use sse_broadcaster::SseBroadcaster;
pub use types::{JobDispatchArgs, JobDispatchResult, JobResult};
