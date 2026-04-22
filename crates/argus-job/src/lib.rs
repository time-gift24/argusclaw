//! argus-job: Job execution module for argusclaw.
//!
//! Provides JobManager for dispatching background jobs from agents via the
//! unified pipe, and job lifecycle management.

pub mod error;
pub mod job_manager;
pub mod types;

pub use error::JobError;
pub use job_manager::{JobLookup, JobManager};
pub use types::{JobExecutionRequest, RecoveredChildJob};
