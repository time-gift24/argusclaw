use std::sync::Arc;

use agent::db::llm::{
    LlmProviderId, LlmProviderKind, LlmProviderRecord, LlmProviderRepository, SecretString,
};
use agent::db::sqlite::{SqliteLlmProviderRepository, migrate};
use agent::llm::LLMManager;
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
async fn llm_manager_lists_provider_summaries_for_user_selection() {
    let (_temp_dir, _pool, repository) = setup_repository().await;
    repository
        .upsert_provider(&build_record("openai", "OpenAI", true))
        .await
        .expect("openai provider should be stored");
    repository
        .upsert_provider(&build_record("deepseek", "DeepSeek", false))
        .await
        .expect("deepseek provider should be stored");

    let manager = LLMManager::new(Arc::new(repository));
    let providers = manager
        .list_providers()
        .await
        .expect("list providers should succeed");

    assert_eq!(providers.len(), 2);
    assert_eq!(providers[0].display_name, "DeepSeek");
    assert_eq!(providers[1].display_name, "OpenAI");
    assert!(providers[1].is_default);
}

#[cfg(feature = "openai-compatible")]
#[tokio::test]
async fn llm_manager_builds_a_provider_from_the_stored_default_configuration() {
    let (_temp_dir, _pool, repository) = setup_repository().await;
    repository
        .upsert_provider(&build_record("openai", "OpenAI", true))
        .await
        .expect("openai provider should be stored");

    let manager = LLMManager::new(Arc::new(repository));
    let provider = manager
        .get_default_provider()
        .await
        .expect("default provider should be built");

    assert_eq!(provider.model_name(), "gpt-4o-mini");
}

#[cfg(feature = "dev")]
#[tokio::test]
async fn llm_manager_exposes_dev_passthrough_for_provider_records() {
    let (_temp_dir, _pool, repository) = setup_repository().await;
    let openai = build_record("openai", "OpenAI", true);
    let deepseek = build_record("deepseek", "DeepSeek", false);
    let manager = LLMManager::new(Arc::new(repository));

    manager
        .upsert_provider(openai.clone())
        .await
        .expect("openai provider should be stored");
    manager
        .upsert_provider(deepseek.clone())
        .await
        .expect("deepseek provider should be stored");

    manager
        .set_default_provider(&deepseek.id)
        .await
        .expect("default provider should be updated");

    let stored = manager
        .get_provider_record(&deepseek.id)
        .await
        .expect("provider record should be fetched");
    let default_provider = manager
        .get_default_provider_record()
        .await
        .expect("default provider record should be fetched");

    assert_eq!(stored.id, deepseek.id);
    assert_eq!(default_provider.id, deepseek.id);
}

#[cfg(feature = "dev")]
#[tokio::test]
async fn llm_manager_can_import_multiple_providers() {
    let (_temp_dir, _pool, repository) = setup_repository().await;
    let manager = LLMManager::new(Arc::new(repository));
    let openai = build_record("openai", "OpenAI", false);
    let deepseek = build_record("deepseek", "DeepSeek", true);

    manager
        .import_providers(vec![openai.clone(), deepseek.clone()])
        .await
        .expect("providers should import");

    let providers = manager
        .list_providers()
        .await
        .expect("providers should list after import");
    let default_provider = manager
        .get_default_provider_record()
        .await
        .expect("default provider should exist after import");

    assert_eq!(providers.len(), 2);
    assert_eq!(default_provider.id, deepseek.id);
}
