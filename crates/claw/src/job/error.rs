use thiserror::Error;

use crate::db::DbError;

/// Errors that can occur in the job module.
#[derive(Debug, Error)]
pub enum JobError {
    #[error(transparent)]
    Database(#[from] DbError),

    #[error("job `{id}` not found")]
    NotFound { id: String },

    #[error("invalid job type: {value}")]
    InvalidJobType { value: String },
}
