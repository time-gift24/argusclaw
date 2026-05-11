use std::str::FromStr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use argus_protocol::{AgentId, ProviderId, SessionId, ThreadId, UserId};
use argus_repository::error::DbError;
use argus_repository::traits::JobRepository;
use argus_repository::types::{
    JobId, JobRecord, JobStatus, ScheduledMessageContext, ScheduledMessageRunContext,
};
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use chrono_tz::Tz;
use croner::{errors::CronError, Cron};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::sync::Notify;
use tokio::time::MissedTickBehavior;

#[derive(Debug, Error)]
pub enum ScheduledMessageError {
    #[error("invalid cron expression: {0}")]
    InvalidCron(String),

    #[error("invalid timezone: {0}")]
    InvalidTimezone(String),

    #[error("cron expression has no next run")]
    NoNextRun,

    #[error("invalid scheduled message context: {0}")]
    InvalidContext(String),

    #[error("scheduled message has no target thread")]
    MissingThread,

    #[error("scheduled message dispatch failed: {0}")]
    Dispatch(String),

    #[error("scheduled message repository operation failed: {0}")]
    Repository(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateScheduledMessageRequest {
    pub owner_user_id: UserId,
    pub template_id: AgentId,
    pub provider_id: Option<ProviderId>,
    pub model: Option<String>,
    pub name: String,
    pub prompt: String,
    pub cron_expr: Option<String>,
    pub scheduled_at: Option<String>,
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateScheduledMessageRequest {
    pub template_id: AgentId,
    pub provider_id: Option<ProviderId>,
    pub model: Option<String>,
    pub name: String,
    pub prompt: String,
    pub cron_expr: Option<String>,
    pub scheduled_at: Option<String>,
    pub timezone: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledMessageSummary {
    pub id: String,
    pub name: String,
    pub status: JobStatus,
    pub template_id: AgentId,
    pub provider_id: Option<ProviderId>,
    pub model: Option<String>,
    pub last_session_id: Option<SessionId>,
    pub last_thread_id: Option<ThreadId>,
    pub run_history: Vec<ScheduledMessageRunSummary>,
    pub prompt: String,
    pub cron_expr: Option<String>,
    pub scheduled_at: Option<String>,
    pub timezone: Option<String>,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ScheduledMessageRunSummary {
    pub session_id: SessionId,
    pub thread_id: ThreadId,
    pub created_at: String,
}

impl From<DbError> for ScheduledMessageError {
    fn from(error: DbError) -> Self {
        Self::Repository(error.to_string())
    }
}

pub fn next_cron_run(
    expr: &str,
    timezone: Option<&str>,
    now: DateTime<Utc>,
) -> Result<DateTime<Utc>, ScheduledMessageError> {
    let cron = Cron::from_str(expr).map_err(invalid_cron)?;
    let timezone = parse_timezone(timezone)?;
    let local_now = now.with_timezone(&timezone);

    cron.find_next_occurrence(&local_now, false)
        .map(|next| next.with_timezone(&Utc))
        .map_err(next_run_error)
}

fn parse_timezone(timezone: Option<&str>) -> Result<Tz, ScheduledMessageError> {
    match timezone
        .map(str::trim)
        .filter(|timezone| !timezone.is_empty())
    {
        Some(timezone) => timezone
            .parse()
            .map_err(|_| ScheduledMessageError::InvalidTimezone(timezone.to_owned())),
        None => Ok(Tz::UTC),
    }
}

fn invalid_cron(error: CronError) -> ScheduledMessageError {
    ScheduledMessageError::InvalidCron(error.to_string())
}

fn next_run_error(error: CronError) -> ScheduledMessageError {
    match error {
        CronError::TimeSearchLimitExceeded => ScheduledMessageError::NoNextRun,
        error => ScheduledMessageError::InvalidCron(error.to_string()),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScheduledMessageDelivery {
    pub session_id: SessionId,
    pub thread_id: ThreadId,
}

#[async_trait]
pub trait ScheduledMessageDispatcher: Send + Sync {
    async fn deliver_scheduled_message(
        &self,
        name: String,
        owner_user_id: UserId,
        template_id: AgentId,
        provider_id: Option<ProviderId>,
        model: Option<String>,
        prompt: String,
    ) -> Result<ScheduledMessageDelivery, ScheduledMessageError>;
}

pub struct CronScheduler {
    job_repository: Arc<dyn JobRepository>,
    dispatcher: Arc<dyn ScheduledMessageDispatcher>,
    notify: Arc<Notify>,
    background_loop_started: AtomicBool,
}

impl CronScheduler {
    pub fn new(
        job_repository: Arc<dyn JobRepository>,
        dispatcher: Arc<dyn ScheduledMessageDispatcher>,
    ) -> Self {
        Self {
            job_repository,
            dispatcher,
            notify: Arc::new(Notify::new()),
            background_loop_started: AtomicBool::new(false),
        }
    }

    pub fn start_background_loop(self: &Arc<Self>) {
        self.start_background_loop_with_interval(Duration::from_secs(30));
    }

    pub fn is_background_loop_started(&self) -> bool {
        self.background_loop_started.load(Ordering::SeqCst)
    }

    fn start_background_loop_with_interval(self: &Arc<Self>, poll_interval: Duration) {
        if self.background_loop_started.swap(true, Ordering::SeqCst) {
            return;
        }

        let scheduler = Arc::clone(self);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(poll_interval);
            interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

            loop {
                tokio::select! {
                    _ = interval.tick() => {}
                    _ = scheduler.notify.notified() => {}
                }

                if let Err(error) = scheduler.run_due_once(Utc::now()).await {
                    tracing::warn!(error = %error, "scheduled message scheduler tick failed");
                }
            }
        });
    }

    pub fn notify_changed(&self) {
        self.notify.notify_waiters();
    }

    pub async fn run_due_once(&self, now: DateTime<Utc>) -> Result<usize, ScheduledMessageError> {
        let jobs = self
            .job_repository
            .find_due_cron_jobs(&now.to_rfc3339())
            .await?;
        let mut delivered = 0;

        for job in jobs {
            match self.run_one_due_job(job, now).await {
                Ok(true) => delivered += 1,
                Ok(false) => {}
                Err(error) => {
                    tracing::warn!(error = %error, "scheduled message run failed");
                }
            }
        }

        Ok(delivered)
    }

    pub async fn run_job_now(
        &self,
        job: JobRecord,
        now: DateTime<Utc>,
    ) -> Result<bool, ScheduledMessageError> {
        self.run_one_due_job(job, now).await
    }

    async fn run_one_due_job(
        &self,
        job: JobRecord,
        now: DateTime<Utc>,
    ) -> Result<bool, ScheduledMessageError> {
        if !self
            .job_repository
            .claim_cron_job(&job.id, &now.to_rfc3339())
            .await?
        {
            return Ok(false);
        }

        let mut context = match parse_context(&job) {
            Ok(context) if context.enabled => context,
            Ok(mut context) => {
                context.last_error = Some("scheduled message is disabled".to_string());
                self.pause_job(&job.id, &context, now).await?;
                return Ok(false);
            }
            Err(error) => {
                let fallback = context_with_error(error.to_string());
                self.pause_job(&job.id, &fallback, now).await?;
                return Ok(false);
            }
        };

        let owner_user_id = match context.owner_user_id.as_deref().map(UserId::parse) {
            Some(Ok(user_id)) => user_id,
            Some(Err(error)) => {
                context.last_error = Some(format!("invalid owner user id: {error}"));
                self.pause_job(&job.id, &context, now).await?;
                return Ok(false);
            }
            None => {
                context.last_error = Some("scheduled message has no owner user".to_string());
                self.pause_job(&job.id, &context, now).await?;
                return Ok(false);
            }
        };
        let template_id = match context.template_id {
            Some(template_id) => AgentId::new(template_id),
            None => {
                context.last_error = Some("scheduled message has no template".to_string());
                self.pause_job(&job.id, &context, now).await?;
                return Ok(false);
            }
        };
        let provider_id = context.provider_id.map(ProviderId::new);

        let delivery = match self
            .dispatcher
            .deliver_scheduled_message(
                job.name.clone(),
                owner_user_id,
                template_id,
                provider_id,
                context.model.clone(),
                job.prompt,
            )
            .await
        {
            Ok(delivery) => delivery,
            Err(error) => {
                context.last_error = Some(error.to_string());
                self.update_job(&job.id, JobStatus::Failed, None, Utc::now(), &context)
                    .await?;
                return Err(error);
            }
        };

        let completed_at = Utc::now();
        context.last_error = None;
        context.last_session_id = Some(delivery.session_id.to_string());
        context.last_thread_id = Some(delivery.thread_id.to_string());
        context.run_history.push(ScheduledMessageRunContext {
            session_id: delivery.session_id.to_string(),
            thread_id: delivery.thread_id.to_string(),
            created_at: completed_at.to_rfc3339(),
        });
        let next_scheduled_at = match job.cron_expr.as_deref() {
            Some(expr) if !expr.trim().is_empty() => {
                match next_cron_run(expr, context.timezone.as_deref(), completed_at) {
                    Ok(next) => Some(next.to_rfc3339()),
                    Err(error) => {
                        context.last_error = Some(error.to_string());
                        self.update_job(&job.id, JobStatus::Failed, None, completed_at, &context)
                            .await?;
                        return Err(error);
                    }
                }
            }
            _ => None,
        };
        let status = if next_scheduled_at.is_some() {
            JobStatus::Pending
        } else {
            JobStatus::Succeeded
        };

        self.update_job(
            &job.id,
            status,
            next_scheduled_at.as_deref(),
            completed_at,
            &context,
        )
        .await?;

        Ok(true)
    }

    async fn pause_job(
        &self,
        id: &JobId,
        context: &ScheduledMessageContext,
        now: DateTime<Utc>,
    ) -> Result<(), ScheduledMessageError> {
        self.update_job(id, JobStatus::Paused, None, now, context)
            .await
    }

    async fn update_job(
        &self,
        id: &JobId,
        status: JobStatus,
        scheduled_at: Option<&str>,
        now: DateTime<Utc>,
        context: &ScheduledMessageContext,
    ) -> Result<(), ScheduledMessageError> {
        let context_json = serde_json::to_string(context)
            .map_err(|error| ScheduledMessageError::InvalidContext(error.to_string()))?;
        let updated = self
            .job_repository
            .update_cron_after_run(
                id,
                status,
                scheduled_at,
                &now.to_rfc3339(),
                Some(&context_json),
            )
            .await?;
        if !updated {
            tracing::info!(
                job_id = %id,
                status = %status,
                "Skipped scheduled message completion update because job is no longer running"
            );
        }
        Ok(())
    }
}

fn parse_context(job: &JobRecord) -> Result<ScheduledMessageContext, ScheduledMessageError> {
    let context = job
        .context
        .as_deref()
        .ok_or_else(|| ScheduledMessageError::InvalidContext("missing context".to_string()))?;
    serde_json::from_str(context)
        .map_err(|error| ScheduledMessageError::InvalidContext(error.to_string()))
}

fn context_with_error(error: String) -> ScheduledMessageContext {
    let mut context = ScheduledMessageContext::new();
    context.enabled = false;
    context.last_error = Some(error);
    context
}

#[cfg(test)]
mod tests {
    use super::{next_cron_run, ScheduledMessageError};
    use chrono::{DateTime, Utc};

    #[test]
    fn next_cron_run_respects_timezone() {
        let now = parse_utc("2026-05-07T00:00:00Z");

        let next = next_cron_run("0 9 * * *", Some("Asia/Shanghai"), now).unwrap();

        assert_eq!(next, parse_utc("2026-05-07T01:00:00Z"));
    }

    #[test]
    fn next_cron_run_rejects_invalid_cron() {
        let now = parse_utc("2026-05-07T00:00:00Z");

        let error = next_cron_run("not a cron", Some("Asia/Shanghai"), now).unwrap_err();

        assert!(matches!(error, ScheduledMessageError::InvalidCron(_)));
    }

    #[test]
    fn next_cron_run_rejects_invalid_timezone() {
        let now = parse_utc("2026-05-07T00:00:00Z");

        let error = next_cron_run("0 9 * * *", Some("Not/AZone"), now).unwrap_err();

        assert!(matches!(error, ScheduledMessageError::InvalidTimezone(_)));
    }

    #[test]
    fn blank_timezone_defaults_to_utc() {
        let now = parse_utc("2026-05-07T00:00:00Z");

        let next = next_cron_run("0 9 * * *", Some("   "), now).unwrap();

        assert_eq!(next, parse_utc("2026-05-07T09:00:00Z"));
    }

    fn parse_utc(value: &str) -> DateTime<Utc> {
        value.parse().unwrap()
    }
}

#[cfg(test)]
mod scheduler_tests {
    use std::sync::Mutex;

    use argus_protocol::{AgentId, ProviderId, UserId};
    use argus_repository::types::{JobRecord, JobResult, JobType};

    use super::*;

    type RecordedDelivery = (
        String,
        UserId,
        AgentId,
        Option<ProviderId>,
        Option<String>,
        String,
    );

    #[derive(Default)]
    struct RecordingDispatcher {
        messages: Mutex<Vec<RecordedDelivery>>,
        next_session_id: Mutex<Option<SessionId>>,
        next_thread_id: Mutex<Option<ThreadId>>,
    }

    #[async_trait]
    impl ScheduledMessageDispatcher for RecordingDispatcher {
        async fn deliver_scheduled_message(
            &self,
            name: String,
            owner_user_id: UserId,
            template_id: AgentId,
            provider_id: Option<ProviderId>,
            model: Option<String>,
            prompt: String,
        ) -> Result<ScheduledMessageDelivery, ScheduledMessageError> {
            self.messages.lock().unwrap().push((
                name,
                owner_user_id,
                template_id,
                provider_id,
                model,
                prompt,
            ));
            Ok(ScheduledMessageDelivery {
                session_id: self
                    .next_session_id
                    .lock()
                    .unwrap()
                    .take()
                    .unwrap_or_default(),
                thread_id: self
                    .next_thread_id
                    .lock()
                    .unwrap()
                    .take()
                    .unwrap_or_default(),
            })
        }
    }

    #[derive(Debug)]
    struct CronUpdate {
        id: JobId,
        status: JobStatus,
        scheduled_at: Option<String>,
        context: Option<String>,
    }

    #[derive(Default)]
    struct FakeJobRepository {
        due_jobs: Mutex<Vec<JobRecord>>,
        claimed: Mutex<Vec<JobId>>,
        updates: Mutex<Vec<CronUpdate>>,
    }

    #[async_trait]
    impl JobRepository for FakeJobRepository {
        async fn create(&self, _job: &JobRecord) -> Result<(), DbError> {
            unimplemented!("not needed by scheduler tests")
        }

        async fn get(&self, _id: &JobId) -> Result<Option<JobRecord>, DbError> {
            unimplemented!("not needed by scheduler tests")
        }

        async fn update_status(
            &self,
            _id: &JobId,
            _status: JobStatus,
            _started_at: Option<&str>,
            _finished_at: Option<&str>,
        ) -> Result<(), DbError> {
            unimplemented!("not needed by scheduler tests")
        }

        async fn update_result(&self, _id: &JobId, _result: &JobResult) -> Result<(), DbError> {
            unimplemented!("not needed by scheduler tests")
        }

        async fn update_thread_id(
            &self,
            _id: &JobId,
            _thread_id: &ThreadId,
        ) -> Result<(), DbError> {
            unimplemented!("not needed by scheduler tests")
        }

        async fn find_ready_jobs(&self, _limit: usize) -> Result<Vec<JobRecord>, DbError> {
            unimplemented!("not needed by scheduler tests")
        }

        async fn find_due_cron_jobs(&self, _now: &str) -> Result<Vec<JobRecord>, DbError> {
            Ok(std::mem::take(&mut *self.due_jobs.lock().unwrap()))
        }

        async fn claim_cron_job(&self, id: &JobId, _started_at: &str) -> Result<bool, DbError> {
            let mut claimed = self.claimed.lock().unwrap();
            if claimed.iter().any(|claimed_id| claimed_id == id) {
                return Ok(false);
            }
            claimed.push(id.clone());
            Ok(true)
        }

        async fn update_cron_after_run(
            &self,
            id: &JobId,
            status: JobStatus,
            scheduled_at: Option<&str>,
            _finished_at: &str,
            context: Option<&str>,
        ) -> Result<bool, DbError> {
            self.updates.lock().unwrap().push(CronUpdate {
                id: id.clone(),
                status,
                scheduled_at: scheduled_at.map(str::to_string),
                context: context.map(str::to_string),
            });
            Ok(true)
        }

        async fn update_cron_definition(
            &self,
            _job: &JobRecord,
            _context: Option<&str>,
        ) -> Result<bool, DbError> {
            unimplemented!("not needed by scheduler tests")
        }

        async fn list_cron_jobs(
            &self,
            _include_paused: bool,
            _thread_id: Option<&ThreadId>,
        ) -> Result<Vec<JobRecord>, DbError> {
            unimplemented!("not needed by scheduler tests")
        }

        async fn trigger_cron_job_now(&self, _id: &JobId, _next: &str) -> Result<bool, DbError> {
            unimplemented!("not needed by scheduler tests")
        }

        async fn list_by_group(&self, _group_id: &str) -> Result<Vec<JobRecord>, DbError> {
            unimplemented!("not needed by scheduler tests")
        }

        async fn delete(&self, _id: &JobId) -> Result<bool, DbError> {
            unimplemented!("not needed by scheduler tests")
        }
    }

    #[tokio::test]
    async fn run_due_once_delivers_user_input_and_advances_recurring_job() {
        let owner_user_id = UserId::new();
        let session_id = SessionId::new();
        let thread_id = ThreadId::new();
        let now = DateTime::parse_from_rfc3339("2026-05-07T00:00:00Z")
            .unwrap()
            .with_timezone(&Utc);
        let job = scheduled_job("cron-deliver", owner_user_id);
        let repo = Arc::new(FakeJobRepository {
            due_jobs: Mutex::new(vec![job]),
            claimed: Mutex::default(),
            updates: Mutex::default(),
        });
        let dispatcher = Arc::new(RecordingDispatcher {
            messages: Mutex::default(),
            next_session_id: Mutex::new(Some(session_id)),
            next_thread_id: Mutex::new(Some(thread_id)),
        });
        let scheduler = CronScheduler::new(repo.clone(), dispatcher.clone());

        let delivered = scheduler.run_due_once(now).await.unwrap();

        assert_eq!(delivered, 1);
        assert_eq!(
            dispatcher.messages.lock().unwrap().as_slice(),
            &[(
                "Scheduled wake up".to_string(),
                owner_user_id,
                AgentId::new(1),
                Some(ProviderId::new(2)),
                Some("alpha".to_string()),
                "Wake up".to_string()
            )]
        );
        let updates = repo.updates.lock().unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].id.as_ref(), "cron-deliver");
        assert_eq!(updates[0].status, JobStatus::Pending);
        let next = DateTime::parse_from_rfc3339(
            updates[0]
                .scheduled_at
                .as_deref()
                .expect("recurring job should have a next schedule"),
        )
        .unwrap()
        .with_timezone(&Utc);
        assert!(
            next > now,
            "next schedule should be after the original due time"
        );
        let context: ScheduledMessageContext =
            serde_json::from_str(updates[0].context.as_deref().unwrap()).unwrap();
        assert_eq!(context.owner_user_id, Some(owner_user_id.to_string()));
        assert_eq!(context.last_session_id, Some(session_id.to_string()));
        assert_eq!(context.last_thread_id, Some(thread_id.to_string()));
        assert_eq!(context.run_history.len(), 1);
        assert_eq!(context.run_history[0].session_id, session_id.to_string());
        assert_eq!(context.run_history[0].thread_id, thread_id.to_string());
        assert_eq!(context.last_error, None);
    }

    fn scheduled_job(id: &str, owner_user_id: UserId) -> JobRecord {
        let context = ScheduledMessageContext {
            owner_user_id: Some(owner_user_id.to_string()),
            template_id: Some(1),
            provider_id: Some(2),
            model: Some("alpha".to_string()),
            last_session_id: None,
            last_thread_id: None,
            run_history: Vec::new(),
            enabled: true,
            timezone: Some("Asia/Shanghai".to_string()),
            last_error: Some("old error".to_string()),
        };
        JobRecord {
            id: JobId::new(id),
            job_type: JobType::Cron,
            name: "Scheduled wake up".to_string(),
            status: JobStatus::Pending,
            agent_id: AgentId::new(1),
            context: Some(serde_json::to_string(&context).unwrap()),
            prompt: "Wake up".to_string(),
            thread_id: None,
            group_id: None,
            depends_on: vec![],
            cron_expr: Some("0 9 * * *".to_string()),
            scheduled_at: Some("2026-05-07T00:00:00Z".to_string()),
            started_at: None,
            finished_at: None,
            parent_job_id: None,
            result: None,
        }
    }
}
