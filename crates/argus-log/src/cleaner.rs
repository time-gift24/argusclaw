use std::sync::Arc;

use crate::models::CleanupReport;
use crate::repository::TurnLogRepository;
use argus_protocol::Result;

/// LRU-based log cleaner that keeps logs only for the most recent N threads.
pub struct LogCleaner<R: TurnLogRepository> {
    repository: Arc<R>,
    max_threads: i64,
}

impl<R: TurnLogRepository> LogCleaner<R> {
    pub fn new(repository: Arc<R>, max_threads: i64) -> Self {
        Self {
            repository,
            max_threads,
        }
    }

    /// Clean up old turn logs, keeping only logs for the most recent `max_threads` threads.
    pub async fn cleanup(&self) -> Result<CleanupReport> {
        let keep_ids = self
            .repository
            .get_active_thread_ids(self.max_threads)
            .await?;
        let deleted = self.repository.delete_except(&keep_ids).await?;
        Ok(CleanupReport {
            deleted_count: deleted,
        })
    }

    /// Get the maximum number of threads to keep logs for.
    pub fn max_threads(&self) -> i64 {
        self.max_threads
    }
}
