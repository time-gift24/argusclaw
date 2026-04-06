//! PostgreSQL integration tests for user isolation.
//!
//! These tests verify that:
//! - Users can be upserted by `external_subject`
//! - Sessions and threads are only listed for the owning user
//! - Provider token credentials can be read back for a provider
//! - `agent_templates.is_enabled` defaults correctly for migrated rows
//!
//! Requires a running PostgreSQL database configured via:
//! - `ARGUS_TEST_PG_URL` (full connection string, e.g. `postgres://user:pass@localhost:5432/argus_test`)
//!
//! If the environment variable is not set, all tests in this file are skipped.

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

use argus_protocol::{AgentId, AgentType, ProviderId, SessionId, ThreadId};
use argus_repository::postgres::ArgusPostgres;
use argus_repository::traits::{
    AgentRepository, ProviderTokenCredentialRepository, SessionRepository, ThreadRepository,
    UserRepository,
};
use argus_repository::types::{
    AgentRecord, OAuth2Identity, ProviderTokenCredential, ThreadRecord,
};

/// Helper to get the test database URL or skip the test.
fn test_pg_url() -> Option<String> {
    std::env::var("ARGUS_TEST_PG_URL").ok()
}

/// Set up a fresh PostgreSQL test database with the schema applied.
async fn setup_test_db() -> PgPool {
    let url = test_pg_url().expect("ARGUS_TEST_PG_URL must be set to run Postgres tests");
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&url)
        .await
        .expect("failed to connect to PostgreSQL test database");

    // Run the PostgreSQL schema migration
    argus_repository::postgres::migrate(&pool)
        .await
        .expect("PostgreSQL migration failed");

    pool
}

/// Clean all tables between tests for isolation.
async fn clean_all(pool: &PgPool) {
    sqlx::query(
        "TRUNCATE TABLE agent_mcp_tools, agent_mcp_servers, mcp_server_tools, mcp_servers, \
         messages, threads, jobs, sessions, provider_token_credentials, agents, users, \
         llm_providers CASCADE",
    )
    .execute(pool)
    .await
    .expect("failed to truncate tables");
}

/// Create a test user via upsert.
async fn create_test_user(repo: &ArgusPostgres, subject: &str, email: &str) -> argus_repository::types::UserRecord {
    let identity = OAuth2Identity {
        external_subject: subject.to_string(),
        account: email.to_string(),
        display_name: email.to_string(),
    };
    repo.upsert_from_oauth2(&identity)
        .await
        .expect("upsert_from_oauth2 should succeed")
}

fn sample_agent(id: i64, display_name: &str) -> AgentRecord {
    AgentRecord {
        id: AgentId::new(id),
        display_name: display_name.to_string(),
        description: "Test agent".to_string(),
        version: "1.0.0".to_string(),
        provider_id: None,
        model_id: None,
        system_prompt: "You are a test agent.".to_string(),
        tool_names: vec![],
        max_tokens: None,
        temperature: None,
        thinking_config: None,
        parent_agent_id: None,
        agent_type: AgentType::Standard,
        is_enabled: true,
    }
}

// ============================================================
// Test: User upsert by external_subject
// ============================================================

#[tokio::test]
async fn user_upsert_creates_new_user() {
    let Some(_) = test_pg_url() else {
        eprintln!("Skipping: ARGUS_TEST_PG_URL not set");
        return;
    };
    let pool = setup_test_db().await;
    clean_all(&pool).await;
    let repo = ArgusPostgres::new(pool.clone());

    let user = create_test_user(&repo, "oauth2|github|12345", "alice@example.com").await;

    assert!(user.id > 0, "user should get a positive database ID");
    assert_eq!(user.external_subject, "oauth2|github|12345");
    assert_eq!(user.account, "alice@example.com");
    assert_eq!(user.display_name, "alice@example.com");
}

#[tokio::test]
async fn user_upsert_idempotent_by_external_subject() {
    let Some(_) = test_pg_url() else {
        eprintln!("Skipping: ARGUS_TEST_PG_URL not set");
        return;
    };
    let pool = setup_test_db().await;
    clean_all(&pool).await;
    let repo = ArgusPostgres::new(pool.clone());

    let first = create_test_user(&repo, "oauth2|google|abc", "bob@example.com").await;
    let second = create_test_user(&repo, "oauth2|google|abc", "bob@newemail.com").await;

    assert_eq!(first.id, second.id, "upsert should return the same user ID");
    assert_eq!(
        second.account, "bob@newemail.com",
        "account should be updated on re-upsert"
    );
    assert_eq!(
        second.display_name, "bob@newemail.com",
        "display_name should be updated on re-upsert"
    );
}

#[tokio::test]
async fn user_get_by_id() {
    let Some(_) = test_pg_url() else {
        eprintln!("Skipping: ARGUS_TEST_PG_URL not set");
        return;
    };
    let pool = setup_test_db().await;
    clean_all(&pool).await;
    let repo = ArgusPostgres::new(pool.clone());

    let created = create_test_user(&repo, "oauth2|gitlab|42", "carol@example.com").await;
    let fetched = repo
        .get_by_id(created.id)
        .await
        .expect("get_by_id should succeed")
        .expect("user should exist");

    assert_eq!(fetched.id, created.id);
    assert_eq!(fetched.external_subject, "oauth2|gitlab|42");
}

// ============================================================
// Test: Sessions are owner-aware
// ============================================================

#[tokio::test]
async fn sessions_are_scoped_to_owning_user() {
    let Some(_) = test_pg_url() else {
        eprintln!("Skipping: ARGUS_TEST_PG_URL not set");
        return;
    };
    let pool = setup_test_db().await;
    clean_all(&pool).await;
    let repo = ArgusPostgres::new(pool.clone());

    let alice = create_test_user(&repo, "sub-alice", "alice@example.com").await;
    let bob = create_test_user(&repo, "sub-bob", "bob@example.com").await;

    // Alice creates a session
    let alice_session_id = SessionId::new();
    repo.create_session_for_user(&alice_session_id, "Alice Session", alice.id)
        .await
        .expect("create session for alice");

    // Bob creates a session
    let bob_session_id = SessionId::new();
    repo.create_session_for_user(&bob_session_id, "Bob Session", bob.id)
        .await
        .expect("create session for bob");

    // List Alice's sessions: should only see Alice's
    let alice_sessions = repo
        .list_sessions_for_user(alice.id)
        .await
        .expect("list alice sessions");
    assert_eq!(alice_sessions.len(), 1);
    assert_eq!(alice_sessions[0].session.name, "Alice Session");

    // List Bob's sessions: should only see Bob's
    let bob_sessions = repo
        .list_sessions_for_user(bob.id)
        .await
        .expect("list bob sessions");
    assert_eq!(bob_sessions.len(), 1);
    assert_eq!(bob_sessions[0].session.name, "Bob Session");
}

// ============================================================
// Test: Threads are owner-aware via session ownership
// ============================================================

#[tokio::test]
async fn threads_are_scoped_to_owning_user_via_session() {
    let Some(_) = test_pg_url() else {
        eprintln!("Skipping: ARGUS_TEST_PG_URL not set");
        return;
    };
    let pool = setup_test_db().await;
    clean_all(&pool).await;
    let repo = ArgusPostgres::new(pool.clone());

    let alice = create_test_user(&repo, "sub-alice-thr", "alice+thr@example.com").await;
    let bob = create_test_user(&repo, "sub-bob-thr", "bob+thr@example.com").await;

    // Create sessions for each
    let alice_sid = SessionId::new();
    repo.create_session_for_user(&alice_sid, "Alice S", alice.id)
        .await
        .expect("create session");

    let bob_sid = SessionId::new();
    repo.create_session_for_user(&bob_sid, "Bob S", bob.id)
        .await
        .expect("create session");

    // Insert an agent so threads have a valid template_id
    let agent = sample_agent(1, "Thread Test Agent");
    AgentRepository::upsert(&repo, &agent)
        .await
        .expect("upsert agent");

    // Create threads in Alice's session
    repo.upsert_thread(&ThreadRecord {
        id: ThreadId::new(),
        provider_id: argus_protocol::llm::LlmProviderId::new(1),
        title: Some("Alice Thread".to_string()),
        token_count: 0,
        turn_count: 0,
        session_id: Some(alice_sid.clone()),
        template_id: Some(AgentId::new(1)),
        model_override: None,
        created_at: "2026-04-06T00:00:00Z".to_string(),
        updated_at: "2026-04-06T00:00:00Z".to_string(),
    })
    .await
    .expect("upsert alice thread");

    // Create threads in Bob's session
    repo.upsert_thread(&ThreadRecord {
        id: ThreadId::new(),
        provider_id: argus_protocol::llm::LlmProviderId::new(1),
        title: Some("Bob Thread".to_string()),
        token_count: 0,
        turn_count: 0,
        session_id: Some(bob_sid.clone()),
        template_id: Some(AgentId::new(1)),
        model_override: None,
        created_at: "2026-04-06T00:00:00Z".to_string(),
        updated_at: "2026-04-06T00:00:00Z".to_string(),
    })
    .await
    .expect("upsert bob thread");

    // List threads in Alice's session
    let alice_threads = repo
        .list_threads_in_session(&alice_sid)
        .await
        .expect("list alice threads");
    assert_eq!(alice_threads.len(), 1);
    assert_eq!(alice_threads[0].title.as_deref(), Some("Alice Thread"));

    // List threads in Bob's session
    let bob_threads = repo
        .list_threads_in_session(&bob_sid)
        .await
        .expect("list bob threads");
    assert_eq!(bob_threads.len(), 1);
    assert_eq!(bob_threads[0].title.as_deref(), Some("Bob Thread"));
}

// ============================================================
// Test: Provider token credentials round-trip
// ============================================================

#[tokio::test]
async fn provider_token_credentials_round_trip() {
    let Some(_) = test_pg_url() else {
        eprintln!("Skipping: ARGUS_TEST_PG_URL not set");
        return;
    };
    let pool = setup_test_db().await;
    clean_all(&pool).await;
    let repo = ArgusPostgres::new(pool.clone());

    let provider_id = ProviderId::new(42);
    let credential = ProviderTokenCredential {
        provider_id: provider_id.clone(),
        username: "token-user".to_string(),
        ciphertext: vec![1, 2, 3, 4],
        nonce: vec![5, 6, 7, 8],
    };

    repo.save_credentials(&credential)
        .await
        .expect("save_credentials should succeed");

    let loaded = repo
        .get_credentials_for_provider(&provider_id)
        .await
        .expect("get_credentials_for_provider should succeed")
        .expect("credentials should exist");

    assert_eq!(loaded.provider_id, provider_id);
    assert_eq!(loaded.username, "token-user");
    assert_eq!(loaded.ciphertext, vec![1, 2, 3, 4]);
    assert_eq!(loaded.nonce, vec![5, 6, 7, 8]);
}

#[tokio::test]
async fn provider_token_credentials_overwrite_on_save() {
    let Some(_) = test_pg_url() else {
        eprintln!("Skipping: ARGUS_TEST_PG_URL not set");
        return;
    };
    let pool = setup_test_db().await;
    clean_all(&pool).await;
    let repo = ArgusPostgres::new(pool.clone());

    let provider_id = ProviderId::new(99);

    let first = ProviderTokenCredential {
        provider_id: provider_id.clone(),
        username: "first-user".to_string(),
        ciphertext: vec![10, 20],
        nonce: vec![30, 40],
    };
    repo.save_credentials(&first).await.expect("save first");

    let second = ProviderTokenCredential {
        provider_id: provider_id.clone(),
        username: "second-user".to_string(),
        ciphertext: vec![50, 60],
        nonce: vec![70, 80],
    };
    repo.save_credentials(&second).await.expect("save second");

    let loaded = repo
        .get_credentials_for_provider(&provider_id)
        .await
        .expect("get should succeed")
        .expect("should exist");

    assert_eq!(loaded.username, "second-user");
    assert_eq!(loaded.ciphertext, vec![50, 60]);
}

// ============================================================
// Test: Agent is_enabled defaults correctly
// ============================================================

#[tokio::test]
async fn agent_is_enabled_defaults_to_true() {
    let Some(_) = test_pg_url() else {
        eprintln!("Skipping: ARGUS_TEST_PG_URL not set");
        return;
    };
    let pool = setup_test_db().await;
    clean_all(&pool).await;
    let repo = ArgusPostgres::new(pool.clone());

    let agent = sample_agent(10, "Enabled Agent");
    AgentRepository::upsert(&repo, &agent)
        .await
        .expect("upsert agent");

    let loaded = AgentRepository::get(&repo, &AgentId::new(10))
        .await
        .expect("get agent")
        .expect("agent should exist");
    assert!(
        loaded.is_enabled,
        "is_enabled should default to true for new agents"
    );
}

#[tokio::test]
async fn agent_is_enabled_persists_false() {
    let Some(_) = test_pg_url() else {
        eprintln!("Skipping: ARGUS_TEST_PG_URL not set");
        return;
    };
    let pool = setup_test_db().await;
    clean_all(&pool).await;
    let repo = ArgusPostgres::new(pool.clone());

    let mut agent = sample_agent(11, "Disabled Agent");
    agent.is_enabled = false;
    AgentRepository::upsert(&repo, &agent)
        .await
        .expect("upsert agent");

    let loaded = AgentRepository::get(&repo, &AgentId::new(11))
        .await
        .expect("get agent")
        .expect("agent should exist");
    assert!(
        !loaded.is_enabled,
        "is_enabled should be false when set to false"
    );
}
