use thiserror::Error;

use crate::db::DbError;
use crate::error::AgentError;

#[derive(Debug, Error)]
pub enum SchedulerError {
    #[error(transparent)]
    Database(#[from] DbError),

    #[error(transparent)]
    Agent(#[from] AgentError),

    #[error("failed to dispatch job `{job_id}`: {reason}")]
    DispatchFailed { job_id: String, reason: String },

    #[error("cron expression parse failed for job `{job_id}`: {reason}")]
    CronParseFailed { job_id: String, reason: String },
}
