use argus_protocol::{AgentId, SessionId, ThreadId};
use argus_repository::traits::AgentRunRepository;
use argus_repository::types::{AgentRunId, AgentRunRecord, AgentRunStatus};
use argus_repository::{ArgusSqlite, migrate};

async fn repository() -> ArgusSqlite {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("in-memory sqlite should connect");
    migrate(&pool).await.expect("migrations should run");
    ArgusSqlite::new(pool)
}

fn run_record() -> AgentRunRecord {
    AgentRunRecord {
        id: AgentRunId::new(),
        agent_id: AgentId::new(42),
        session_id: SessionId::new(),
        thread_id: ThreadId::new(),
        prompt: "Summarize the repository boundary".to_string(),
        status: AgentRunStatus::Queued,
        result: None,
        error: None,
        created_at: "2026-04-25T00:00:00Z".to_string(),
        updated_at: "2026-04-25T00:00:00Z".to_string(),
        completed_at: None,
    }
}

#[tokio::test]
async fn agent_run_repository_persists_run_and_state_updates() {
    let repo = repository().await;
    let run = run_record();

    AgentRunRepository::insert_agent_run(&repo, &run)
        .await
        .expect("run should insert");

    let stored = AgentRunRepository::get_agent_run(&repo, &run.id)
        .await
        .expect("run should load")
        .expect("run should exist");
    assert_eq!(stored, run);

    AgentRunRepository::update_agent_run_status(
        &repo,
        &run.id,
        AgentRunStatus::Completed,
        Some("Done"),
        None,
        Some("2026-04-25T00:00:03Z"),
        "2026-04-25T00:00:03Z",
    )
    .await
    .expect("run status should update");

    let completed = AgentRunRepository::get_agent_run(&repo, &run.id)
        .await
        .expect("run should load")
        .expect("run should exist");
    assert_eq!(completed.status, AgentRunStatus::Completed);
    assert_eq!(completed.result.as_deref(), Some("Done"));
    assert_eq!(completed.error, None);
    assert_eq!(
        completed.completed_at.as_deref(),
        Some("2026-04-25T00:00:03Z")
    );
}

#[tokio::test]
async fn agent_run_repository_persists_failure_state() {
    let repo = repository().await;
    let run = run_record();

    AgentRunRepository::insert_agent_run(&repo, &run)
        .await
        .expect("run should insert");
    AgentRunRepository::update_agent_run_status(
        &repo,
        &run.id,
        AgentRunStatus::Failed,
        None,
        Some("provider failed"),
        Some("2026-04-25T00:00:04Z"),
        "2026-04-25T00:00:04Z",
    )
    .await
    .expect("run status should update");

    let failed = AgentRunRepository::get_agent_run(&repo, &run.id)
        .await
        .expect("run should load")
        .expect("run should exist");
    assert_eq!(failed.status, AgentRunStatus::Failed);
    assert_eq!(failed.result, None);
    assert_eq!(failed.error.as_deref(), Some("provider failed"));
    assert_eq!(failed.completed_at.as_deref(), Some("2026-04-25T00:00:04Z"));
}
