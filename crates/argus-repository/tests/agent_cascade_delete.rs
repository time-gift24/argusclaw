use argus_protocol::{AgentId, AgentRecord, LlmProviderId, SessionId, ThreadId};
use argus_repository::traits::{AgentRepository, JobRepository, SessionRepository, ThreadRepository};
use argus_repository::types::{JobId, JobRecord, JobStatus, JobType, ThreadRecord};
use argus_repository::{ArgusSqlite, migrate};

async fn repository() -> ArgusSqlite {
    let pool = sqlx::SqlitePool::connect("sqlite::memory:")
        .await
        .expect("in-memory sqlite should connect");
    migrate(&pool).await.expect("migrations should run");
    ArgusSqlite::new(pool)
}

fn agent(display_name: &str) -> AgentRecord {
    AgentRecord {
        id: AgentId::new(0),
        display_name: display_name.to_string(),
        description: format!("{display_name} description"),
        version: "1.0.0".to_string(),
        provider_id: None,
        model_id: None,
        system_prompt: format!("{display_name} system prompt"),
        tool_names: vec![],
        subagent_names: vec![],
        max_tokens: None,
        temperature: None,
        thinking_config: None,
    }
}

fn job(id: &str, agent_id: AgentId) -> JobRecord {
    JobRecord {
        id: JobId::new(id),
        job_type: JobType::Standalone,
        name: format!("{id} name"),
        status: JobStatus::Pending,
        agent_id,
        context: None,
        prompt: format!("{id} prompt"),
        thread_id: None,
        group_id: None,
        depends_on: vec![],
        cron_expr: None,
        scheduled_at: None,
        started_at: None,
        finished_at: None,
        parent_job_id: None,
        result: None,
    }
}

fn thread(id: ThreadId, session_id: SessionId, template_id: AgentId, title: &str) -> ThreadRecord {
    ThreadRecord {
        id,
        provider_id: LlmProviderId::new(1),
        title: Some(title.to_string()),
        token_count: 0,
        turn_count: 0,
        session_id: Some(session_id),
        template_id: Some(template_id),
        model_override: None,
        created_at: "2026-05-06T00:00:00Z".to_string(),
        updated_at: "2026-05-06T00:00:00Z".to_string(),
    }
}

#[tokio::test]
async fn cascade_delete_removes_agent_jobs_threads_and_empty_sessions() {
    let repo = repository().await;

    let agent_id = AgentRepository::upsert(&repo, &agent("Delete Me"))
        .await
        .expect("agent should upsert");
    let unrelated_agent_id = AgentRepository::upsert(&repo, &agent("Keep Me"))
        .await
        .expect("unrelated agent should upsert");

    JobRepository::create(&repo, &job("job-to-delete", agent_id))
        .await
        .expect("job should insert");

    let single_thread_session_id = SessionId::new();
    SessionRepository::create(&repo, &single_thread_session_id, "single matching thread")
        .await
        .expect("single-thread session should insert");
    let deleted_thread_id = ThreadId::new();
    ThreadRepository::upsert_thread(
        &repo,
        &thread(
            deleted_thread_id,
            single_thread_session_id,
            agent_id,
            "deleted thread",
        ),
    )
    .await
    .expect("matching thread should insert");

    let mixed_session_id = SessionId::new();
    SessionRepository::create(&repo, &mixed_session_id, "mixed threads")
        .await
        .expect("mixed session should insert");
    let deleted_mixed_thread_id = ThreadId::new();
    ThreadRepository::upsert_thread(
        &repo,
        &thread(
            deleted_mixed_thread_id,
            mixed_session_id,
            agent_id,
            "deleted mixed thread",
        ),
    )
    .await
    .expect("matching mixed thread should insert");
    let kept_thread_id = ThreadId::new();
    ThreadRepository::upsert_thread(
        &repo,
        &thread(
            kept_thread_id,
            mixed_session_id,
            unrelated_agent_id,
            "kept mixed thread",
        ),
    )
    .await
    .expect("unrelated mixed thread should insert");

    let report = AgentRepository::delete_with_associations(&repo, &agent_id)
        .await
        .expect("cascade delete should succeed");

    assert!(report.agent_deleted);
    assert_eq!(report.deleted_job_count, 1);
    assert_eq!(report.deleted_thread_count, 2);
    assert_eq!(report.deleted_session_count, 1);

    assert!(
        AgentRepository::get(&repo, &agent_id)
            .await
            .expect("agent lookup should succeed")
            .is_none()
    );
    assert!(
        JobRepository::get(&repo, &JobId::new("job-to-delete"))
            .await
            .expect("job lookup should succeed")
            .is_none()
    );
    assert!(
        ThreadRepository::get_thread(&repo, &deleted_thread_id)
            .await
            .expect("deleted thread lookup should succeed")
            .is_none()
    );
    assert!(
        ThreadRepository::get_thread(&repo, &deleted_mixed_thread_id)
            .await
            .expect("deleted mixed thread lookup should succeed")
            .is_none()
    );
    assert!(
        SessionRepository::get(&repo, &single_thread_session_id)
            .await
            .expect("single-thread session lookup should succeed")
            .is_none()
    );
    assert!(
        SessionRepository::get(&repo, &mixed_session_id)
            .await
            .expect("mixed session lookup should succeed")
            .is_some()
    );
    assert_eq!(
        ThreadRepository::get_thread(&repo, &kept_thread_id)
            .await
            .expect("kept thread lookup should succeed")
            .expect("kept thread should remain")
            .template_id,
        Some(unrelated_agent_id)
    );
}
