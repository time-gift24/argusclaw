//! argus-job: Job execution module for argusclaw.
//!
//! Provides JobManager for dispatching background jobs from agents via the
//! unified pipe, and job lifecycle management.

pub mod error;
pub mod job_manager;
pub mod scheduler_tool;
pub mod thread_pool;
pub mod types;
pub mod workflow_manager;

pub use error::JobError;
pub use job_manager::{JobLookup, JobManager};
pub use scheduler_tool::SchedulerTool;
pub use thread_pool::ThreadPool;
pub use types::{
    GetWorkflowProgressResult, JobResult, SchedulerToolArgs, StartWorkflowResult,
    SubagentSummary,
};
pub use workflow_manager::{
    AppendWorkflowNode, InstantiateWorkflowInput, WorkflowExecutionProgress, WorkflowManager,
};
