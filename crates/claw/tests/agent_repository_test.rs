#![cfg(feature = "dev")]

//! Integration tests for SqliteAgentRepository.

use claw::{AgentId, AgentRecord, AgentRepository, SqliteAgentRepository, connect, migrate};

fn create_test_record(id: &str, provider_id: &str) -> AgentRecord {
    AgentRecord {
        id: AgentId::new(id),
        display_name: format!("Agent {id}"),
        description: "Integration test agent".to_string(),
        version: "1.0.0".to_string(),
        provider_id: provider_id.to_string(),
        system_prompt: "You are a test agent.".to_string(),
        tool_names: vec!["tool1".to_string(), "tool2".to_string()],
        max_tokens: Some(2000),
        temperature: Some(0.5),
    }
}

async fn setup_test_db() -> SqliteAgentRepository {
    let pool = connect("sqlite::memory:").await.unwrap();
    migrate(&pool).await.unwrap();
    SqliteAgentRepository::new(pool)
}

/// Insert a minimal LLM provider to satisfy foreign key constraints.
async fn insert_test_provider(pool: &sqlx::SqlitePool, provider_id: &str) {
    sqlx::query(
        r#"INSERT INTO llm_providers (id, kind, display_name, base_url, model, encrypted_api_key, api_key_nonce, is_default)
           VALUES (?1, 'openai_compatible', 'Test Provider', 'https://api.example.com', 'gpt-4', X'00', X'00', 0)"#,
    )
    .bind(provider_id)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn upsert_and_get_agent() {
    let repo = setup_test_db().await;
    insert_test_provider(repo.pool(), "provider-1").await;

    let record = create_test_record("test-agent-1", "provider-1");
    repo.upsert(&record).await.unwrap();

    let retrieved = repo.get(&AgentId::new("test-agent-1")).await.unwrap();
    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.id.as_ref(), "test-agent-1");
    assert_eq!(retrieved.tool_names, vec!["tool1", "tool2"]);
}

#[tokio::test]
async fn get_returns_none_for_missing() {
    let repo = setup_test_db().await;

    let result = repo.get(&AgentId::new("missing")).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn upsert_updates_existing() {
    let repo = setup_test_db().await;
    insert_test_provider(repo.pool(), "provider-1").await;

    let mut record = create_test_record("update-test", "provider-1");
    repo.upsert(&record).await.unwrap();

    record.display_name = "Updated Name".to_string();
    record.version = "2.0.0".to_string();
    repo.upsert(&record).await.unwrap();

    let retrieved = repo
        .get(&AgentId::new("update-test"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(retrieved.display_name, "Updated Name");
    assert_eq!(retrieved.version, "2.0.0");
}

#[tokio::test]
async fn list_returns_summaries() {
    let repo = setup_test_db().await;
    insert_test_provider(repo.pool(), "provider-1").await;

    repo.upsert(&create_test_record("list-1", "provider-1"))
        .await
        .unwrap();
    repo.upsert(&create_test_record("list-2", "provider-1"))
        .await
        .unwrap();

    let summaries = repo.list().await.unwrap();
    assert_eq!(summaries.len(), 2);

    let ids: Vec<&str> = summaries.iter().map(|s| s.id.as_ref()).collect();
    assert!(ids.contains(&"list-1"));
    assert!(ids.contains(&"list-2"));
}

#[tokio::test]
async fn delete_removes_agent() {
    let repo = setup_test_db().await;
    insert_test_provider(repo.pool(), "provider-1").await;

    repo.upsert(&create_test_record("delete-test", "provider-1"))
        .await
        .unwrap();

    let deleted = repo.delete(&AgentId::new("delete-test")).await.unwrap();
    assert!(deleted);

    let result = repo.get(&AgentId::new("delete-test")).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn delete_returns_false_for_missing() {
    let repo = setup_test_db().await;

    let deleted = repo.delete(&AgentId::new("missing")).await.unwrap();
    assert!(!deleted);
}

#[tokio::test]
async fn temperature_precision_preserved() {
    let repo = setup_test_db().await;
    insert_test_provider(repo.pool(), "provider-1").await;

    let mut record = create_test_record("temp-test", "provider-1");
    record.temperature = Some(0.73);
    repo.upsert(&record).await.unwrap();

    let retrieved = repo.get(&AgentId::new("temp-test")).await.unwrap().unwrap();
    // Allow small floating point error (stored as integer / 100)
    assert!((retrieved.temperature.unwrap() - 0.73).abs() < 0.01);
}

#[tokio::test]
async fn summary_excludes_large_fields() {
    let repo = setup_test_db().await;
    insert_test_provider(repo.pool(), "provider-1").await;

    let record = create_test_record("summary-test", "provider-1");
    repo.upsert(&record).await.unwrap();

    let summaries = repo.list().await.unwrap();
    let summary = summaries
        .iter()
        .find(|s| s.id.as_ref() == "summary-test")
        .unwrap();

    // Summary should have display_name but not system_prompt
    // (This is enforced at compile time by the type system)
    assert_eq!(summary.display_name, "Agent summary-test");
}
