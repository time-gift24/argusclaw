use agent::db::llm::{
    LlmProviderId, LlmProviderKind, LlmProviderRecord, LlmProviderRepository, SecretString,
};
use agent::db::sqlite::{SqliteLlmProviderRepository, migrate};
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteConnectOptions;

fn build_record(id: &str, display_name: &str, is_default: bool) -> LlmProviderRecord {
    LlmProviderRecord {
        id: LlmProviderId::new(id),
        kind: LlmProviderKind::OpenAiCompatible,
        display_name: display_name.to_string(),
        base_url: format!("https://{id}.example.com/v1"),
        api_key: SecretString::new(format!("sk-{id}")),
        model: "gpt-4o-mini".to_string(),
        is_default,
    }
}

async fn setup_repository() -> (tempfile::TempDir, SqlitePool, SqliteLlmProviderRepository) {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let database_path = temp_dir.path().join("argusclaw.db");
    let options = SqliteConnectOptions::new()
        .filename(&database_path)
        .create_if_missing(true);
    let pool = SqlitePool::connect_with(options)
        .await
        .expect("sqlite pool should connect");
    migrate(&pool).await.expect("migrations should run");

    let repository = SqliteLlmProviderRepository::new_with_key_material(
        pool.clone(),
        b"fixed-test-key".to_vec(),
    );

    (temp_dir, pool, repository)
}

#[tokio::test]
async fn sqlite_repository_round_trips_provider_records_and_encrypts_api_keys() {
    let (_temp_dir, pool, repository) = setup_repository().await;
    let record = build_record("openai", "OpenAI", true);

    repository
        .upsert_provider(&record)
        .await
        .expect("provider should be stored");

    let stored = repository
        .get_provider(&record.id)
        .await
        .expect("query should succeed")
        .expect("provider should exist");

    assert_eq!(stored.id, record.id);
    assert_eq!(stored.display_name, "OpenAI");
    assert_eq!(stored.api_key.expose_secret(), "sk-openai");

    let encrypted_api_key: Vec<u8> =
        sqlx::query_scalar("select encrypted_api_key from llm_providers where id = ?1")
            .bind("openai")
            .fetch_one(&pool)
            .await
            .expect("encrypted api key should be stored");

    assert_ne!(encrypted_api_key, b"sk-openai");
}

#[tokio::test]
async fn sqlite_repository_reassigns_the_default_provider() {
    let (_temp_dir, _pool, repository) = setup_repository().await;
    let first = build_record("openai", "OpenAI", true);
    let second = build_record("deepseek", "DeepSeek", true);

    repository
        .upsert_provider(&first)
        .await
        .expect("first provider should be stored");
    repository
        .upsert_provider(&second)
        .await
        .expect("second provider should be stored");

    let default_provider = repository
        .get_default_provider()
        .await
        .expect("default query should succeed")
        .expect("default provider should exist");
    let first_provider = repository
        .get_provider(&first.id)
        .await
        .expect("first provider query should succeed")
        .expect("first provider should exist");
    let providers = repository
        .list_providers()
        .await
        .expect("list should succeed");

    assert_eq!(default_provider.id, second.id);
    assert!(!first_provider.is_default);
    assert_eq!(providers.len(), 2);
}

#[cfg(feature = "dev")]
#[tokio::test]
async fn sqlite_repository_can_set_the_default_provider_by_id() {
    let (_temp_dir, _pool, repository) = setup_repository().await;
    let first = build_record("openai", "OpenAI", true);
    let second = build_record("deepseek", "DeepSeek", false);

    repository
        .upsert_provider(&first)
        .await
        .expect("first provider should be stored");
    repository
        .upsert_provider(&second)
        .await
        .expect("second provider should be stored");

    repository
        .set_default_provider(&second.id)
        .await
        .expect("default provider should be updated");

    let default_provider = repository
        .get_default_provider()
        .await
        .expect("default query should succeed")
        .expect("default provider should exist");
    let first_provider = repository
        .get_provider(&first.id)
        .await
        .expect("first provider query should succeed")
        .expect("first provider should exist");

    assert_eq!(default_provider.id, second.id);
    assert!(!first_provider.is_default);
}
