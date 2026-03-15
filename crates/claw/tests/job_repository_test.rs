#![cfg(feature = "dev")]

//! Integration tests for the JobRepository.
//!
//! These tests verify:
//! - Job CRUD operations
//! - Ready job scheduling logic (dependencies)
//! - Cron job filtering
//! - Status transition guards
//! - Group filtering
//! - Thread ID association

use claw::{
    AgentId, JobId, JobRecord, JobRepository, JobType, SqliteJobRepository, ThreadId,
    WorkflowStatus, connect, migrate,
};

async fn setup() -> SqliteJobRepository {
    let pool = connect("sqlite::memory:").await.unwrap();
    migrate(&pool).await.unwrap();
    // Insert dummy provider + agent for FK constraints
    sqlx::query(
        "INSERT INTO llm_providers (id, kind, display_name, base_url, model, encrypted_api_key, api_key_nonce) VALUES ('prov-1', 'openai', 'Test', 'http://localhost', 'gpt-4', X'00', X'00')"
    )
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO agents (id, display_name, provider_id, system_prompt) VALUES ('agent-1', 'Test Agent', 'prov-1', 'You are a test agent')"
    )
    .execute(&pool)
    .await
    .unwrap();
    SqliteJobRepository::new(pool)
}

/// Helper to create a minimal job record for testing.
fn make_job(id: &str, agent_id: &str, name: &str, prompt: &str) -> JobRecord {
    JobRecord {
        id: JobId::new(id),
        job_type: JobType::Standalone,
        name: name.to_string(),
        status: WorkflowStatus::Pending,
        agent_id: AgentId::new(agent_id),
        context: None,
        prompt: prompt.to_string(),
        thread_id: None,
        group_id: None,
        depends_on: vec![],
        cron_expr: None,
        scheduled_at: None,
        started_at: None,
        finished_at: None,
    }
}

#[tokio::test]
async fn test_create_and_get_standalone_job() {
    let repo = setup().await;

    let job = make_job("job-1", "agent-1", "Test Job", "Do something");
    repo.create(&job).await.unwrap();

    let fetched = repo.get(&job.id).await.unwrap();
    assert!(fetched.is_some(), "Job should be found");

    let fetched = fetched.unwrap();
    assert_eq!(fetched.id.as_ref(), "job-1", "ID should match");
    assert_eq!(fetched.name, "Test Job", "Name should match");
    assert_eq!(
        fetched.job_type,
        JobType::Standalone,
        "Job type should be Standalone"
    );
    assert_eq!(
        fetched.status,
        WorkflowStatus::Pending,
        "Status should be Pending"
    );
    assert_eq!(
        fetched.agent_id.as_ref(),
        "agent-1",
        "Agent ID should match"
    );
    assert_eq!(fetched.prompt, "Do something", "Prompt should match");
    assert!(
        fetched.thread_id.is_none(),
        "Thread ID should be None initially"
    );
    assert!(fetched.depends_on.is_empty(), "Depends on should be empty");
}

#[tokio::test]
async fn test_find_ready_jobs_no_dependencies() {
    let repo = setup().await;

    // Create a standalone job with no dependencies
    let job = make_job("job-ready", "agent-1", "Ready Job", "Do it");
    repo.create(&job).await.unwrap();

    let ready = repo.find_ready_jobs(10).await.unwrap();
    assert_eq!(ready.len(), 1, "One job should be ready");
    assert_eq!(
        ready[0].id.as_ref(),
        "job-ready",
        "Ready job should be the one we created"
    );
}

#[tokio::test]
async fn test_find_ready_jobs_respects_dependencies() {
    let repo = setup().await;

    // Create job A (no dependencies)
    let mut job_a = make_job("job-a", "agent-1", "Job A", "Do A");
    job_a.job_type = JobType::Workflow;
    repo.create(&job_a).await.unwrap();

    // Create job B (depends on A)
    let mut job_b = make_job("job-b", "agent-1", "Job B", "Do B");
    job_b.job_type = JobType::Workflow;
    job_b.depends_on = vec![JobId::new("job-a")];
    repo.create(&job_b).await.unwrap();

    // Initially, only A should be ready
    let ready = repo.find_ready_jobs(10).await.unwrap();
    assert_eq!(ready.len(), 1, "Only job A should be ready initially");
    assert_eq!(ready[0].id.as_ref(), "job-a", "Job A should be ready");

    // Mark job A as succeeded
    repo.update_status(
        &job_a.id,
        WorkflowStatus::Succeeded,
        Some("2024-01-01T10:00:00Z"),
        Some("2024-01-01T10:05:00Z"),
    )
    .await
    .unwrap();

    // Now B should be ready (A is done, B's dependency is satisfied)
    // Note: A is no longer "pending" so it won't appear in ready jobs
    let ready = repo.find_ready_jobs(10).await.unwrap();
    assert_eq!(ready.len(), 1, "Only pending jobs should be returned");
    assert_eq!(
        ready[0].id.as_ref(),
        "job-b",
        "Job B should be ready after A succeeds"
    );
}

#[tokio::test]
async fn test_find_ready_jobs_skips_cron_templates() {
    let repo = setup().await;

    // Create a cron job (template, not a concrete instance)
    let mut cron_job = make_job("cron-job", "agent-1", "Cron Job", "Do cron");
    cron_job.job_type = JobType::Cron;
    cron_job.cron_expr = Some("0 * * * *".to_string());
    cron_job.scheduled_at = Some("2024-01-01T00:00:00Z".to_string());
    repo.create(&cron_job).await.unwrap();

    // Create a standalone job
    let standalone_job = make_job("standalone-job", "agent-1", "Standalone", "Do it");
    repo.create(&standalone_job).await.unwrap();

    // Cron jobs should NOT appear in ready jobs (they are templates, not executable)
    let ready = repo.find_ready_jobs(10).await.unwrap();
    assert_eq!(ready.len(), 1, "Only standalone job should be ready");
    assert_eq!(
        ready[0].id.as_ref(),
        "standalone-job",
        "Cron job should be excluded from ready jobs"
    );
}

#[tokio::test]
async fn test_update_status_prevents_terminal_transition() {
    let repo = setup().await;

    let job = make_job("job-terminal", "agent-1", "Terminal Job", "Do it");
    repo.create(&job).await.unwrap();

    // Mark as succeeded
    repo.update_status(
        &job.id,
        WorkflowStatus::Succeeded,
        Some("2024-01-01T10:00:00Z"),
        Some("2024-01-01T10:05:00Z"),
    )
    .await
    .unwrap();

    // Try to transition back to running (should fail - terminal state protection)
    let result = repo
        .update_status(
            &job.id,
            WorkflowStatus::Running,
            Some("2024-01-01T10:10:00Z"),
            None,
        )
        .await;

    assert!(
        result.is_err(),
        "Should fail when trying to update from terminal state"
    );

    // Verify status is still succeeded (not changed)
    let fetched = repo.get(&job.id).await.unwrap().unwrap();
    assert_eq!(
        fetched.status,
        WorkflowStatus::Succeeded,
        "Status should remain succeeded - terminal state transition should be prevented"
    );
}

#[tokio::test]
async fn test_list_by_group() {
    let repo = setup().await;

    // Create jobs with different groups
    let mut job1 = make_job("job-g1-1", "agent-1", "Job in Group A", "Do 1");
    job1.group_id = Some("group-a".to_string());
    repo.create(&job1).await.unwrap();

    let mut job2 = make_job("job-g1-2", "agent-1", "Job in Group A", "Do 2");
    job2.group_id = Some("group-a".to_string());
    repo.create(&job2).await.unwrap();

    let mut job3 = make_job("job-g2-1", "agent-1", "Job in Group B", "Do 3");
    job3.group_id = Some("group-b".to_string());
    repo.create(&job3).await.unwrap();

    // Filter by group-a
    let group_a_jobs = repo.list_by_group("group-a").await.unwrap();
    assert_eq!(group_a_jobs.len(), 2, "Should have 2 jobs in group-a");
    let names: Vec<_> = group_a_jobs.iter().map(|j| j.name.as_str()).collect();
    assert!(
        names.contains(&"Job in Group A"),
        "Should contain job from group-a"
    );

    // Filter by group-b
    let group_b_jobs = repo.list_by_group("group-b").await.unwrap();
    assert_eq!(group_b_jobs.len(), 1, "Should have 1 job in group-b");
    assert_eq!(
        group_b_jobs[0].name, "Job in Group B",
        "Should be job from group-b"
    );

    // Filter by non-existent group
    let no_jobs = repo.list_by_group("non-existent").await.unwrap();
    assert_eq!(
        no_jobs.len(),
        0,
        "Should have no jobs in non-existent group"
    );
}

#[tokio::test]
async fn test_delete_job() {
    let repo = setup().await;

    let job = make_job("job-to-delete", "agent-1", "Delete Me", "Do it");
    repo.create(&job).await.unwrap();

    // Verify it exists
    let fetched = repo.get(&job.id).await.unwrap();
    assert!(fetched.is_some(), "Job should exist before deletion");

    // Delete the job
    let deleted = repo.delete(&job.id).await.unwrap();
    assert!(deleted, "Delete should return true for existing job");

    // Verify it's gone
    let fetched = repo.get(&job.id).await.unwrap();
    assert!(fetched.is_none(), "Job should be deleted");

    // Delete non-existent job should return false
    let deleted = repo.delete(&JobId::new("non-existent")).await.unwrap();
    assert!(!deleted, "Delete should return false for non-existent job");
}

#[tokio::test]
async fn test_update_thread_id() {
    let repo = setup().await;

    let job = make_job("job-with-thread", "agent-1", "Thread Job", "Do it");
    repo.create(&job).await.unwrap();

    // Initially no thread
    let fetched = repo.get(&job.id).await.unwrap().unwrap();
    assert!(
        fetched.thread_id.is_none(),
        "Thread ID should be None initially"
    );

    // Update thread ID using parse
    let thread_id = ThreadId::parse("550e8400-e29b-41d4-a716-446655440000").unwrap();
    repo.update_thread_id(&job.id, &thread_id).await.unwrap();

    // Verify thread ID is set
    let fetched = repo.get(&job.id).await.unwrap().unwrap();
    assert!(
        fetched.thread_id.is_some(),
        "Thread ID should be set after update"
    );
    assert_eq!(
        fetched.thread_id.unwrap().to_string(),
        "550e8400-e29b-41d4-a716-446655440000",
        "Thread ID should match"
    );
}

#[tokio::test]
async fn test_depends_on_json_serialization() {
    let repo = setup().await;

    // Create job with dependencies
    let mut job = make_job("job-with-deps", "agent-1", "Dependent Job", "Do it");
    job.job_type = JobType::Workflow;
    job.depends_on = vec![
        JobId::new("dep-1"),
        JobId::new("dep-2"),
        JobId::new("dep-3"),
    ];
    repo.create(&job).await.unwrap();

    // Fetch and verify depends_on deserializes correctly
    let fetched = repo.get(&job.id).await.unwrap().unwrap();
    assert_eq!(fetched.depends_on.len(), 3, "Should have 3 dependencies");
    let dep_ids: Vec<_> = fetched.depends_on.iter().map(|id| id.as_ref()).collect();
    assert!(dep_ids.contains(&"dep-1"), "Should contain dep-1");
    assert!(dep_ids.contains(&"dep-2"), "Should contain dep-2");
    assert!(dep_ids.contains(&"dep-3"), "Should contain dep-3");
}
