//! argus-job: Job execution module for argusclaw.
//!
//! Provides JobManager for dispatching background jobs from agents,
//! SSE-based completion notification, and job lifecycle management.

pub mod dispatch_tool;
pub mod error;
pub mod job_manager;
pub mod sse_broadcaster;
pub mod types;

pub use dispatch_tool::DispatchJobTool;
pub use error::JobError;
pub use job_manager::JobManager;
pub use sse_broadcaster::SseBroadcaster;
pub use types::{JobDispatchArgs, JobDispatchResult, JobResult};
