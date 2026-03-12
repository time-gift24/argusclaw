use std::time::Duration;

pub struct SchedulerConfig {
    pub poll_interval: Duration,
    pub max_concurrent_jobs: usize,
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            poll_interval: Duration::from_secs(5),
            max_concurrent_jobs: 5,
        }
    }
}
