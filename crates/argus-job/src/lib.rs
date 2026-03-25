//! argus-job: Job execution module for argusclaw.
//!
//! Provides JobManager for dispatching background jobs from agents via the
//! unified pipe, and job lifecycle management.

pub mod dispatch_tool;
pub mod error;
pub mod job_manager;
pub mod list_subagents_tool;
pub mod types;

pub use dispatch_tool::DispatchJobTool;
pub use error::JobError;
pub use job_manager::JobManager;
pub use list_subagents_tool::ListSubagentsTool;
pub use types::{JobDispatchArgs, JobResult};
