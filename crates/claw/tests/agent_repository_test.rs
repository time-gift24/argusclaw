#![cfg(feature = "dev")]

//! Integration tests for SqliteAgentRepository.

use claw::{
    AgentId, AgentRecord, AgentRepository, LlmProviderId, SqliteAgentRepository, connect, migrate,
};

fn create_test_record(id: i64, provider_id: Option<LlmProviderId>) -> AgentRecord {
    AgentRecord {
        id: AgentId::new(id),
        display_name: format!("Agent {id}"),
        description: "Integration test agent".to_string(),
        version: "1.0.0".to_string(),
        provider_id,
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
async fn insert_test_provider(pool: &sqlx::SqlitePool, provider_id: i64) {
    sqlx::query(
        r#"INSERT INTO llm_providers (id, kind, display_name, base_url, models, default_model, encrypted_api_key, api_key_nonce, is_default)
           VALUES (?1, 'openai_compatible', 'Test Provider', 'https://api.example.com', '["gpt-4"]', 'gpt-4', X'00', X'00', 0)"#,
    )
    .bind(provider_id)
    .execute(pool)
    .await
    .unwrap();
}

#[tokio::test]
async fn upsert_and_get_agent() {
    let repo = setup_test_db().await;
    insert_test_provider(repo.pool(), 1).await;

    let record = create_test_record(1, Some(LlmProviderId::new(1)));
    repo.upsert(&record).await.unwrap();

    let retrieved = repo.get(&AgentId::new(1)).await.unwrap();
    assert!(retrieved.is_some());
    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.id.into_inner(), 1);
    assert_eq!(retrieved.tool_names, vec!["tool1", "tool2"]);
}

#[tokio::test]
async fn get_returns_none_for_missing() {
    let repo = setup_test_db().await;

    let result = repo.get(&AgentId::new(999)).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn upsert_updates_existing() {
    let repo = setup_test_db().await;
    insert_test_provider(repo.pool(), 1).await;

    let mut record = create_test_record(1, Some(LlmProviderId::new(1)));
    repo.upsert(&record).await.unwrap();

    record.display_name = "Updated Name".to_string();
    record.version = "2.0.0".to_string();
    repo.upsert(&record).await.unwrap();

    let retrieved = repo.get(&AgentId::new(1)).await.unwrap().unwrap();
    assert_eq!(retrieved.display_name, "Updated Name");
    assert_eq!(retrieved.version, "2.0.0");
}

#[tokio::test]
async fn upsert_allows_null_provider_id() {
    let repo = setup_test_db().await;

    let record = create_test_record(1, None);
    repo.upsert(&record).await.unwrap();

    let retrieved = repo.get(&AgentId::new(1)).await.unwrap().unwrap();
    assert!(retrieved.provider_id.is_none());
}

#[tokio::test]
async fn list_returns_summaries() {
    let repo = setup_test_db().await;
    insert_test_provider(repo.pool(), 1).await;

    repo.upsert(&create_test_record(1, Some(LlmProviderId::new(1))))
        .await
        .unwrap();
    repo.upsert(&create_test_record(2, Some(LlmProviderId::new(1))))
        .await
        .unwrap();

    let summaries = repo.list().await.unwrap();
    assert_eq!(summaries.len(), 2);

    let ids: Vec<i64> = summaries.iter().map(|s| s.id.into_inner()).collect();
    assert!(ids.contains(&1));
    assert!(ids.contains(&2));
}

#[tokio::test]
async fn delete_removes_agent() {
    let repo = setup_test_db().await;
    insert_test_provider(repo.pool(), 1).await;

    repo.upsert(&create_test_record(1, Some(LlmProviderId::new(1))))
        .await
        .unwrap();

    let deleted = repo.delete(&AgentId::new(1)).await.unwrap();
    assert!(deleted);

    let result = repo.get(&AgentId::new(1)).await.unwrap();
    assert!(result.is_none());
}

#[tokio::test]
async fn delete_returns_false_for_missing() {
    let repo = setup_test_db().await;

    let deleted = repo.delete(&AgentId::new(999)).await.unwrap();
    assert!(!deleted);
}

#[tokio::test]
async fn temperature_precision_preserved() {
    let repo = setup_test_db().await;
    insert_test_provider(repo.pool(), 1).await;

    let mut record = create_test_record(1, Some(LlmProviderId::new(1)));
    record.temperature = Some(0.73);
    repo.upsert(&record).await.unwrap();

    let retrieved = repo.get(&AgentId::new(1)).await.unwrap().unwrap();
    // Allow small floating point error (stored as integer / 100)
    assert!((retrieved.temperature.unwrap() - 0.73).abs() < 0.01);
}

#[tokio::test]
async fn summary_excludes_large_fields() {
    let repo = setup_test_db().await;
    insert_test_provider(repo.pool(), 1).await;

    let record = create_test_record(1, Some(LlmProviderId::new(1)));
    repo.upsert(&record).await.unwrap();

    let summaries = repo.list().await.unwrap();
    let summary = summaries.iter().find(|s| s.id.into_inner() == 1).unwrap();

    // Summary should have display_name but not system_prompt
    // (This is enforced at compile time by the type system)
    assert_eq!(summary.display_name, "Agent 1");
}
