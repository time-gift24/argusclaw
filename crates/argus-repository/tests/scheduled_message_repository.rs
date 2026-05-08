use argus_protocol::{AgentId, AgentRecord, ThreadId};
use argus_repository::traits::{AgentRepository, JobRepository};
use argus_repository::types::{
    JobId, JobRecord, JobStatus, JobType, ScheduledMessageContext,
};
use argus_repository::{ArgusSqlite, migrate};

async fn repository() -> ArgusSqlite {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("in-memory sqlite should connect");
    migrate(&pool).await.expect("migrations should run");
    let repo = ArgusSqlite::new(pool);

    let agent = AgentRecord {
        id: AgentId::new(1),
        display_name: "Scheduled Message Agent".to_string(),
        description: "A test agent for scheduled message jobs".to_string(),
        version: "1.0.0".to_string(),
        provider_id: None,
        model_id: None,
        system_prompt: "Run scheduled messages.".to_string(),
        tool_names: vec![],
        subagent_names: vec![],
        max_tokens: None,
        temperature: None,
        thinking_config: None,
    };
    AgentRepository::upsert(&repo, &agent)
        .await
        .expect("agent should insert");

    repo
}

fn cron_job(id: &str, status: JobStatus, thread_id: Option<ThreadId>) -> JobRecord {
    JobRecord {
        id: JobId::new(id),
        job_type: JobType::Cron,
        name: format!("Cron {id}"),
        status,
        agent_id: AgentId::new(1),
        context: Some(
            serde_json::to_string(&ScheduledMessageContext::new("session-1"))
                .expect("context should serialize"),
        ),
        prompt: "Send the scheduled message".to_string(),
        thread_id,
        group_id: None,
        depends_on: vec![],
        cron_expr: Some("0 * * * *".to_string()),
        scheduled_at: Some("2026-05-08T01:00:00Z".to_string()),
        started_at: None,
        finished_at: None,
        parent_job_id: None,
        result: None,
    }
}

#[tokio::test]
async fn due_cron_jobs_exclude_paused_records() {
    let repo = repository().await;
    let pending = cron_job("cron-pending", JobStatus::Pending, None);
    let paused = cron_job("cron-paused", JobStatus::Pending, None);

    JobRepository::create(&repo, &pending)
        .await
        .expect("pending cron should insert");
    JobRepository::create(&repo, &paused)
        .await
        .expect("cron to pause should insert");
    JobRepository::update_status(&repo, &paused.id, JobStatus::Paused, None, None)
        .await
        .expect("cron should pause");

    let due = JobRepository::find_due_cron_jobs(&repo, "2026-05-08T01:00:00Z")
        .await
        .expect("due jobs should load");

    assert_eq!(due.len(), 1);
    assert_eq!(due[0].id, pending.id);
    assert_eq!(due[0].status, JobStatus::Pending);
}

#[tokio::test]
async fn cron_job_can_be_claimed_once() {
    let repo = repository().await;
    let job = cron_job("cron-claim", JobStatus::Pending, None);

    JobRepository::create(&repo, &job)
        .await
        .expect("cron should insert");

    assert!(
        JobRepository::claim_cron_job(&repo, &job.id, "2026-05-08T01:00:01Z")
            .await
            .expect("first claim should run")
    );
    assert!(
        !JobRepository::claim_cron_job(&repo, &job.id, "2026-05-08T01:00:02Z")
            .await
            .expect("second claim should run")
    );

    let stored = JobRepository::get(&repo, &job.id)
        .await
        .expect("job should load")
        .expect("job should exist");
    assert_eq!(stored.status, JobStatus::Running);
    assert_eq!(stored.started_at.as_deref(), Some("2026-05-08T01:00:01Z"));
}

#[tokio::test]
async fn update_cron_after_run_updates_next_schedule_and_context() {
    let repo = repository().await;
    let job = cron_job("cron-update", JobStatus::Pending, None);
    let next_context = serde_json::to_string(&ScheduledMessageContext {
        target_session_id: "session-1".to_string(),
        enabled: true,
        timezone: Some("Asia/Shanghai".to_string()),
        last_error: None,
    })
    .expect("context should serialize");

    JobRepository::create(&repo, &job)
        .await
        .expect("cron should insert");
    JobRepository::update_cron_after_run(
        &repo,
        &job.id,
        JobStatus::Pending,
        Some("2026-05-08T02:00:00Z"),
        "2026-05-08T01:00:30Z",
        Some(&next_context),
    )
    .await
    .expect("cron should update after run");

    let stored = JobRepository::get(&repo, &job.id)
        .await
        .expect("job should load")
        .expect("job should exist");
    assert_eq!(stored.status, JobStatus::Pending);
    assert_eq!(
        stored.scheduled_at.as_deref(),
        Some("2026-05-08T02:00:00Z")
    );
    assert_eq!(stored.finished_at.as_deref(), Some("2026-05-08T01:00:30Z"));
    assert_eq!(stored.context.as_deref(), Some(next_context.as_str()));
}

#[tokio::test]
async fn list_cron_jobs_filters_by_thread() {
    let repo = repository().await;
    let thread_a = ThreadId::new();
    let thread_b = ThreadId::new();
    let job_a = cron_job("cron-thread-a", JobStatus::Pending, Some(thread_a));
    let job_b = cron_job("cron-thread-b", JobStatus::Pending, Some(thread_b));

    JobRepository::create(&repo, &job_a)
        .await
        .expect("thread a cron should insert");
    JobRepository::create(&repo, &job_b)
        .await
        .expect("thread b cron should insert");

    let listed = JobRepository::list_cron_jobs(&repo, false, Some(&thread_a))
        .await
        .expect("cron jobs should list");

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, job_a.id);
    assert_eq!(listed[0].thread_id, Some(thread_a));
}
