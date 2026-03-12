use std::sync::Arc;

use dashmap::DashMap;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::agents::AgentManager;
use crate::job::JobRepository;
use crate::workflow::JobId;

use super::config::SchedulerConfig;
use super::error::SchedulerError;

pub struct Scheduler {
    config: SchedulerConfig,
    job_repository: Arc<dyn JobRepository>,
    agent_manager: Arc<AgentManager>,
    running_jobs: DashMap<JobId, JoinHandle<()>>,
    shutdown: CancellationToken,
}

impl Scheduler {
    #[must_use]
    pub fn new(
        config: SchedulerConfig,
        job_repository: Arc<dyn JobRepository>,
        agent_manager: Arc<AgentManager>,
    ) -> Self {
        Self {
            config,
            job_repository,
            agent_manager,
            running_jobs: DashMap::new(),
            shutdown: CancellationToken::new(),
        }
    }

    pub fn shutdown_token(&self) -> CancellationToken {
        self.shutdown.clone()
    }

    pub fn running_count(&self) -> usize {
        self.running_jobs.len()
    }
}
