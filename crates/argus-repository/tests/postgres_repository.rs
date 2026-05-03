use argus_protocol::llm::{LlmProviderId, LlmProviderRepository};
use argus_protocol::{SessionId, ThreadId};
use argus_repository::traits::{SessionRepository, ThreadRepository, UserRepository};
use argus_repository::types::ThreadRecord;
use argus_repository::{ArgusPostgres, connect_postgres, migrate_postgres};
use uuid::Uuid;

const POSTGRES_TEST_URL_ENV: &str = "ARGUS_TEST_POSTGRES_URL";

#[tokio::test]
async fn postgres_migration_and_user_scoped_chat_records() {
    let Some(database_url) = std::env::var(POSTGRES_TEST_URL_ENV).ok() else {
        eprintln!("skipping PostgreSQL repository test: {POSTGRES_TEST_URL_ENV} is not set");
        return;
    };

    let pool = connect_postgres(&database_url)
        .await
        .expect("postgres test database should connect");
    migrate_postgres(&pool)
        .await
        .expect("postgres migrations should run on the test database");

    let repo = ArgusPostgres::new(pool);
    let namespace = Uuid::new_v4();
    let user_a = repo
        .resolve_user(
            &format!("repo-user-a-{namespace}"),
            Some("Repository User A"),
        )
        .await
        .expect("user A should resolve");
    let user_b = repo
        .resolve_user(
            &format!("repo-user-b-{namespace}"),
            Some("Repository User B"),
        )
        .await
        .expect("user B should resolve");

    let session_id = SessionId::new();
    repo.create_for_user(&user_a.id, &session_id, "User A Session")
        .await
        .expect("user A session should be created");

    let provider_id = repo
        .get_default_provider_id()
        .await
        .expect("default provider lookup should succeed")
        .unwrap_or_else(|| LlmProviderId::new(1));
    let thread_id = ThreadId::new();
    let now = chrono::Utc::now().to_rfc3339();
    let thread = ThreadRecord {
        id: thread_id,
        provider_id,
        title: Some("User A Thread".to_string()),
        token_count: 0,
        turn_count: 0,
        session_id: Some(session_id),
        template_id: None,
        model_override: Some("alpha".to_string()),
        created_at: now.clone(),
        updated_at: now,
    };
    repo.upsert_thread_for_user(&user_a.id, &thread)
        .await
        .expect("user A should create a thread in its own session");

    let user_a_sessions = repo
        .list_with_counts_for_user(&user_a.id)
        .await
        .expect("user A sessions should list");
    assert!(
        user_a_sessions
            .iter()
            .any(|candidate| candidate.session.id == session_id)
    );

    let user_b_sessions = repo
        .list_with_counts_for_user(&user_b.id)
        .await
        .expect("user B sessions should list");
    assert!(
        user_b_sessions
            .iter()
            .all(|candidate| candidate.session.id != session_id)
    );

    assert!(
        repo.get_for_user(&user_b.id, &session_id)
            .await
            .expect("user B session lookup should succeed")
            .is_none()
    );
    assert!(
        repo.get_thread_in_session_for_user(&user_b.id, &thread_id, &session_id)
            .await
            .expect("user B thread lookup should succeed")
            .is_none()
    );
    assert!(
        repo.list_threads_in_session_for_user(&user_b.id, &session_id)
            .await
            .expect("user B thread list should succeed")
            .is_empty()
    );
    assert!(
        !repo
            .rename_thread_for_user(&user_b.id, &thread_id, &session_id, Some("User B Rename"))
            .await
            .expect("user B rename should be denied without error")
    );

    assert!(
        repo.rename_thread_for_user(&user_a.id, &thread_id, &session_id, Some("User A Rename"))
            .await
            .expect("user A rename should succeed")
    );
    let renamed = repo
        .get_thread_in_session_for_user(&user_a.id, &thread_id, &session_id)
        .await
        .expect("user A thread lookup should succeed")
        .expect("user A thread should exist");
    assert_eq!(renamed.title.as_deref(), Some("User A Rename"));

    assert!(
        !repo
            .delete_for_user(&user_b.id, &session_id)
            .await
            .expect("user B delete should be denied without error")
    );
    assert!(
        repo.delete_for_user(&user_a.id, &session_id)
            .await
            .expect("user A delete should succeed")
    );
}

#[tokio::test]
async fn postgres_global_session_create_reuses_legacy_user() {
    let Some(database_url) = std::env::var(POSTGRES_TEST_URL_ENV).ok() else {
        eprintln!("skipping PostgreSQL repository test: {POSTGRES_TEST_URL_ENV} is not set");
        return;
    };

    let pool = connect_postgres(&database_url)
        .await
        .expect("postgres test database should connect");
    migrate_postgres(&pool)
        .await
        .expect("postgres migrations should run on the test database");

    let repo = ArgusPostgres::new(pool);
    let first_session = SessionId::new();
    let second_session = SessionId::new();

    repo.create(&first_session, "First legacy session")
        .await
        .expect("first global session should create");
    repo.create(&second_session, "Second legacy session")
        .await
        .expect("second global session should reuse the existing legacy user");

    assert!(
        repo.get(&first_session)
            .await
            .expect("first global session lookup should succeed")
            .is_some()
    );
    assert!(
        repo.get(&second_session)
            .await
            .expect("second global session lookup should succeed")
            .is_some()
    );

    repo.delete(&first_session)
        .await
        .expect("first global session should delete");
    repo.delete(&second_session)
        .await
        .expect("second global session should delete");
}
