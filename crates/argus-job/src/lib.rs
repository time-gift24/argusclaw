//! argus-job: Job execution module for argusclaw.
//!
//! Provides JobManager for dispatching background jobs from agents via the
//! unified pipe, and job lifecycle management.

pub mod dispatch_tool;
pub mod error;
pub mod get_job_result_tool;
pub mod get_workflow_progress_tool;
pub mod job_manager;
pub mod list_subagents_tool;
pub mod thread_pool;
pub mod start_workflow_tool;
pub mod types;
pub mod workflow_manager;

pub use dispatch_tool::DispatchJobTool;
pub use error::JobError;
pub use get_job_result_tool::GetJobResultTool;
pub use get_workflow_progress_tool::GetWorkflowProgressTool;
pub use job_manager::{JobLookup, JobManager};
pub use list_subagents_tool::ListSubagentsTool;
pub use thread_pool::ThreadPool;
pub use start_workflow_tool::StartWorkflowTool;
pub use types::{
    GetJobResultArgs, GetWorkflowProgressArgs, GetWorkflowProgressResult, JobDispatchArgs,
    JobResult, StartWorkflowArgs, StartWorkflowResult,
};
pub use workflow_manager::{
    AppendWorkflowNode, InstantiateWorkflowInput, WorkflowExecutionProgress, WorkflowManager,
};
