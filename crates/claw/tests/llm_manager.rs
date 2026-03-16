#![cfg(feature = "dev")]

use std::collections::HashMap;
use std::sync::Arc;
use std::{
    io::{Read, Write},
    net::TcpListener,
    thread,
};

use claw::{
    LLMManager, LlmModelId, LlmModelRecord, LlmModelRepository, LlmProviderId, LlmProviderKind,
    LlmProviderRecord, LlmProviderRepository, ProviderSecretStatus, ProviderTestStatus,
    SecretString, SqliteLlmModelRepository, SqliteLlmProviderRepository, migrate,
};
use sqlx::SqlitePool;
use sqlx::sqlite::SqliteConnectOptions;

fn build_record(id: &str, display_name: &str, is_default: bool) -> LlmProviderRecord {
    LlmProviderRecord {
        id: LlmProviderId::new(id),
        kind: LlmProviderKind::OpenAiCompatible,
        display_name: display_name.to_string(),
        base_url: format!("https://{id}.example.com/v1"),
        api_key: SecretString::new(format!("sk-{id}")),
        is_default,
        extra_headers: HashMap::new(),
        secret_status: ProviderSecretStatus::Ready,
    }
}

async fn setup_repository() -> (
    tempfile::TempDir,
    SqlitePool,
    SqliteLlmProviderRepository,
    SqliteLlmModelRepository,
) {
    let temp_dir = tempfile::tempdir().expect("tempdir should be created");
    let database_path = temp_dir.path().join("arguswing.db");
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
    let model_repository = SqliteLlmModelRepository::new(pool.clone());

    (temp_dir, pool, repository, model_repository)
}

fn spawn_single_response_server(
    status_line: &str,
    body: &str,
    extra_headers: &[(&str, &str)],
) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").expect("listener should bind");
    let addr = listener.local_addr().expect("listener should have addr");
    let status_line = status_line.to_string();
    let body = body.to_string();
    let extra_headers = extra_headers
        .iter()
        .map(|(name, value)| format!("{name}: {value}\r\n"))
        .collect::<String>();

    thread::spawn(move || {
        let (mut stream, _) = listener.accept().expect("connection should be accepted");
        let mut buffer = [0_u8; 4096];
        let _ = stream.read(&mut buffer);
        let response = format!(
            "HTTP/1.1 {status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n{extra_headers}\r\n{body}",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .expect("response should be written");
    });

    format!("http://{addr}")
}

#[tokio::test]
async fn llm_manager_lists_provider_summaries_for_user_selection() {
    let (_temp_dir, _pool, repository, model_repository) = setup_repository().await;
    repository
        .upsert_provider(&build_record("openai", "OpenAI", true))
        .await
        .expect("openai provider should be stored");
    repository
        .upsert_provider(&build_record("deepseek", "DeepSeek", false))
        .await
        .expect("deepseek provider should be stored");

    let manager = LLMManager::new(Arc::new(repository), Arc::new(model_repository));
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
    let (_temp_dir, _pool, repository, model_repository) = setup_repository().await;
    repository
        .upsert_provider(&build_record("openai", "OpenAI", true))
        .await
        .expect("openai provider should be stored");
    model_repository
        .upsert(&LlmModelRecord {
            id: LlmModelId::new("openai:gpt-4o-mini"),
            provider_id: LlmProviderId::new("openai"),
            name: "gpt-4o-mini".to_string(),
            is_default: true,
        })
        .await
        .expect("model should be stored");

    let manager = LLMManager::new(Arc::new(repository), Arc::new(model_repository));
    let provider = manager
        .get_default_provider()
        .await
        .expect("default provider should be built");

    assert_eq!(provider.model_name(), "gpt-4o-mini");
}

#[cfg(feature = "dev")]
#[tokio::test]
async fn llm_manager_exposes_dev_passthrough_for_provider_records() {
    let (_temp_dir, _pool, repository, model_repository) = setup_repository().await;
    let openai = build_record("openai", "OpenAI", true);
    let deepseek = build_record("deepseek", "DeepSeek", false);
    let manager = LLMManager::new(Arc::new(repository), Arc::new(model_repository));

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
    let (_temp_dir, _pool, repository, model_repository) = setup_repository().await;
    let manager = LLMManager::new(Arc::new(repository), Arc::new(model_repository));
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

#[cfg(feature = "dev")]
#[tokio::test]
async fn llm_manager_deletes_provider_records() {
    let (_temp_dir, _pool, repository, model_repository) = setup_repository().await;
    let manager = LLMManager::new(Arc::new(repository), Arc::new(model_repository));
    let record = build_record("openai", "OpenAI", false);

    manager
        .upsert_provider(record.clone())
        .await
        .expect("provider should be stored");

    let deleted = manager
        .delete_provider(&record.id)
        .await
        .expect("delete should succeed");
    let missing = manager.get_provider_record(&record.id).await;

    assert!(deleted);
    assert!(matches!(
        missing,
        Err(claw::AgentError::ProviderNotFound { id }) if id == "openai"
    ));
}

#[cfg(feature = "openai-compatible")]
#[tokio::test]
async fn llm_manager_reports_successful_provider_connection_tests() {
    let (_temp_dir, _pool, repository, model_repository) = setup_repository().await;
    let base_url = spawn_single_response_server(
        "200 OK",
        r#"{"choices":[{"message":{"content":"OK"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1}}"#,
        &[],
    );
    let manager = LLMManager::new(Arc::new(repository), Arc::new(model_repository));
    let mut record = build_record("openai", "OpenAI", true);
    record.base_url = base_url.clone();

    manager
        .upsert_provider(record.clone())
        .await
        .expect("provider should be stored");
    manager
        .upsert_model(LlmModelRecord {
            id: LlmModelId::new("openai:gpt-4o-mini"),
            provider_id: record.id.clone(),
            name: "gpt-4o-mini".to_string(),
            is_default: true,
        })
        .await
        .expect("model should be stored");

    let result = manager
        .test_provider_connection(&record.id)
        .await
        .expect("test should succeed");

    assert_eq!(result.provider_id, "openai");
    assert_eq!(result.model, "gpt-4o-mini");
    assert_eq!(result.base_url, base_url);
    assert_eq!(result.status, ProviderTestStatus::Success);
    assert_eq!(result.message, "Provider connection test succeeded.");
}

#[cfg(feature = "openai-compatible")]
#[tokio::test]
async fn llm_manager_can_test_unsaved_provider_configurations() {
    let (_temp_dir, _pool, repository, model_repository) = setup_repository().await;
    let base_url = spawn_single_response_server(
        "200 OK",
        r#"{"choices":[{"message":{"content":"OK"},"finish_reason":"stop"}],"usage":{"prompt_tokens":1,"completion_tokens":1}}"#,
        &[],
    );
    let manager = LLMManager::new(Arc::new(repository), Arc::new(model_repository));
    let mut record = build_record("", "Draft Provider", false);
    record.base_url = base_url;
    let model_record = LlmModelRecord {
        id: LlmModelId::new("draft:gpt-4o-mini"),
        provider_id: record.id.clone(),
        name: "gpt-4o-mini".to_string(),
        is_default: true,
    };

    let result = manager
        .test_provider_record(record, &model_record.name)
        .await
        .expect("draft provider test should succeed");

    assert_eq!(result.status, ProviderTestStatus::Success);
    assert_eq!(result.provider_id, "");
}

#[cfg(feature = "openai-compatible")]
#[tokio::test]
async fn llm_manager_maps_auth_failures_for_provider_connection_tests() {
    let (_temp_dir, _pool, repository, model_repository) = setup_repository().await;
    let base_url = spawn_single_response_server("401 Unauthorized", r#"{"error":"bad key"}"#, &[]);
    let manager = LLMManager::new(Arc::new(repository), Arc::new(model_repository));
    let mut record = build_record("openai", "OpenAI", true);
    record.base_url = base_url;

    manager
        .upsert_provider(record.clone())
        .await
        .expect("provider should be stored");
    manager
        .upsert_model(LlmModelRecord {
            id: LlmModelId::new("openai:gpt-4o-mini"),
            provider_id: record.id.clone(),
            name: "gpt-4o-mini".to_string(),
            is_default: true,
        })
        .await
        .expect("model should be stored");

    let result = manager
        .test_provider_connection(&record.id)
        .await
        .expect("test result should be returned");

    assert_eq!(result.status, ProviderTestStatus::AuthFailed);
}

#[cfg(feature = "openai-compatible")]
#[tokio::test]
async fn llm_manager_reports_missing_providers_in_connection_tests() {
    let (_temp_dir, _pool, repository, model_repository) = setup_repository().await;
    let manager = LLMManager::new(Arc::new(repository), Arc::new(model_repository));

    let result = manager
        .test_provider_connection(&LlmProviderId::new("missing"))
        .await
        .expect("missing provider should return a structured result");

    assert_eq!(result.provider_id, "missing");
    assert_eq!(result.status, ProviderTestStatus::ProviderNotFound);
}

#[cfg(feature = "openai-compatible")]
#[tokio::test]
async fn llm_manager_maps_model_availability_failures_for_provider_connection_tests() {
    let (_temp_dir, _pool, repository, model_repository) = setup_repository().await;
    let base_url =
        spawn_single_response_server("404 Not Found", r#"{"error":"model not available"}"#, &[]);
    let manager = LLMManager::new(Arc::new(repository), Arc::new(model_repository));
    let mut record = build_record("openai", "OpenAI", true);
    record.base_url = base_url;

    manager
        .upsert_provider(record.clone())
        .await
        .expect("provider should be stored");
    manager
        .upsert_model(LlmModelRecord {
            id: LlmModelId::new("openai:gpt-4o-mini"),
            provider_id: record.id.clone(),
            name: "gpt-4o-mini".to_string(),
            is_default: true,
        })
        .await
        .expect("model should be stored");

    let result = manager
        .test_provider_connection(&record.id)
        .await
        .expect("test result should be returned");

    assert_eq!(result.status, ProviderTestStatus::ModelNotAvailable);
}

#[cfg(feature = "openai-compatible")]
#[tokio::test]
async fn llm_manager_maps_generic_http_failures_for_provider_connection_tests() {
    let (_temp_dir, _pool, repository, model_repository) = setup_repository().await;
    let base_url = spawn_single_response_server(
        "500 Internal Server Error",
        r#"{"error":"upstream exploded"}"#,
        &[],
    );
    let manager = LLMManager::new(Arc::new(repository), Arc::new(model_repository));
    let mut record = build_record("openai", "OpenAI", true);
    record.base_url = base_url;

    manager
        .upsert_provider(record.clone())
        .await
        .expect("provider should be stored");
    manager
        .upsert_model(LlmModelRecord {
            id: LlmModelId::new("openai:gpt-4o-mini"),
            provider_id: record.id.clone(),
            name: "gpt-4o-mini".to_string(),
            is_default: true,
        })
        .await
        .expect("model should be stored");

    let result = manager
        .test_provider_connection(&record.id)
        .await
        .expect("test result should be returned");

    assert_eq!(result.status, ProviderTestStatus::RequestFailed);
    assert!(result.message.contains("HTTP 500"));
}

#[tokio::test]
async fn llm_manager_lists_provider_summaries_when_some_secrets_require_reentry() {
    let (_temp_dir, pool, repository) = setup_repository().await;
    repository
        .upsert_provider(&build_record("openai", "OpenAI", true))
        .await
        .expect("openai provider should be stored");

    let legacy_repository = SqliteLlmProviderRepository::new_with_key_material(
        pool.clone(),
        b"legacy-test-key".to_vec(),
    );
    legacy_repository
        .upsert_provider(&build_record("legacy", "Legacy", false))
        .await
        .expect("legacy provider should be stored");

    let manager = LLMManager::new(Arc::new(repository));
    let providers = manager
        .list_providers()
        .await
        .expect("provider summaries should still load");

    assert_eq!(providers.len(), 2);
    assert_eq!(providers[0].display_name, "Legacy");
    assert_eq!(
        providers[0].secret_status,
        ProviderSecretStatus::RequiresReentry
    );
    assert_eq!(providers[1].secret_status, ProviderSecretStatus::Ready);
}
