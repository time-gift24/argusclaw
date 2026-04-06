//! Integration tests for provider token credential source decoupling.
//!
//! These tests verify that ProviderManager can construct token-backed
//! providers using the new ProviderTokenCredentialRepository, independent
//! of AccountRepository / desktop login.

use std::collections::HashMap;
use std::sync::Arc;

use argus_protocol::llm::{
    LlmProviderId, LlmProviderKind, LlmProviderRecord, LlmProviderRepository, SecretString,
};
use argus_protocol::ProviderTokenCredential;
use argus_repository::error::DbError;
use argus_repository::traits::ProviderTokenCredentialRepository;
use argus_protocol::ids::ProviderId;
use async_trait::async_trait;
use argus_llm::ProviderManager;

// --- Mocks ---

struct MockProviderRepository {
    record: LlmProviderRecord,
}

#[async_trait]
impl LlmProviderRepository for MockProviderRepository {
    async fn upsert_provider(
        &self,
        _record: &LlmProviderRecord,
    ) -> Result<LlmProviderId, argus_protocol::ArgusError> {
        todo!()
    }
    async fn delete_provider(
        &self,
        _id: &LlmProviderId,
    ) -> Result<bool, argus_protocol::ArgusError> {
        todo!()
    }
    async fn set_default_provider(
        &self,
        _id: &LlmProviderId,
    ) -> Result<(), argus_protocol::ArgusError> {
        todo!()
    }
    async fn get_provider(
        &self,
        _id: &LlmProviderId,
    ) -> Result<Option<LlmProviderRecord>, argus_protocol::ArgusError> {
        Ok(Some(self.record.clone()))
    }
    async fn list_providers(
        &self,
    ) -> Result<Vec<LlmProviderRecord>, argus_protocol::ArgusError> {
        Ok(vec![self.record.clone()])
    }
    async fn get_default_provider(
        &self,
    ) -> Result<Option<LlmProviderRecord>, argus_protocol::ArgusError> {
        Ok(Some(self.record.clone()))
    }
    async fn get_default_provider_id(
        &self,
    ) -> Result<Option<LlmProviderId>, argus_protocol::ArgusError> {
        Ok(Some(self.record.id))
    }
}

struct MockCredentialRepository {
    credential: Option<ProviderTokenCredential>,
}

#[async_trait]
impl ProviderTokenCredentialRepository for MockCredentialRepository {
    async fn get_credentials_for_provider(
        &self,
        _provider_id: &ProviderId,
    ) -> Result<Option<ProviderTokenCredential>, DbError> {
        Ok(self.credential.clone())
    }
    async fn save_credentials(
        &self,
        _credential: &ProviderTokenCredential,
    ) -> Result<(), DbError> {
        unimplemented!()
    }
}

fn make_token_source_record() -> LlmProviderRecord {
    let mut meta_data = HashMap::new();
    meta_data.insert("provider_token_source".to_string(), "true".to_string());

    LlmProviderRecord {
        id: LlmProviderId::new(1),
        kind: LlmProviderKind::OpenAiCompatible,
        display_name: "Token Provider".to_string(),
        base_url: "https://api.example.com/v1".to_string(),
        api_key: SecretString::new("sk-test"),
        models: vec!["gpt-4".to_string()],
        model_config: HashMap::new(),
        default_model: "gpt-4".to_string(),
        is_default: true,
        extra_headers: HashMap::new(),
        secret_status: argus_protocol::ProviderSecretStatus::Ready,
        meta_data,
    }
}

fn make_static_record() -> LlmProviderRecord {
    LlmProviderRecord {
        id: LlmProviderId::new(2),
        kind: LlmProviderKind::OpenAiCompatible,
        display_name: "Static Provider".to_string(),
        base_url: "https://api.example.com/v1".to_string(),
        api_key: SecretString::new("sk-test"),
        models: vec!["gpt-4".to_string()],
        model_config: HashMap::new(),
        default_model: "gpt-4".to_string(),
        is_default: false,
        extra_headers: HashMap::new(),
        secret_status: argus_protocol::ProviderSecretStatus::Ready,
        meta_data: HashMap::new(),
    }
}

#[tokio::test]
async fn provider_manager_builds_static_key_provider_without_credential_repo() {
    // Static API key providers should work as before, no credential repo needed.
    let record = make_static_record();
    let repo = Arc::new(MockProviderRepository {
        record: record.clone(),
    });
    let manager = ProviderManager::new(repo);

    let result = manager.build_provider_with_model(record, "gpt-4").await;
    assert!(result.is_ok(), "static API key should build provider without credential repo");
}

#[tokio::test]
async fn provider_token_source_fails_without_credential_repo() {
    // When meta_data has provider_token_source=true but no credential repo,
    // it should return an error about the missing credential repository.
    let record = make_token_source_record();
    let repo = Arc::new(MockProviderRepository {
        record: record.clone(),
    });
    let manager = ProviderManager::new(repo);

    let result = manager.build_provider_with_model(record, "gpt-4").await;
    assert!(result.is_err(), "should fail without credential repo");
    let err = result.err().expect("already checked");
    assert!(
        err.to_string().contains("credential"),
        "error should mention credential: {err}"
    );
}

#[tokio::test]
async fn provider_token_source_fails_without_stored_credentials() {
    // With a credential repo and cipher but no stored credentials, should fail gracefully.
    let record = make_token_source_record();
    let repo = Arc::new(MockProviderRepository {
        record: record.clone(),
    });
    let key = argus_crypto::StaticKeySource::new(vec![0u8; 32]);
    let cipher = Arc::new(argus_crypto::Cipher::new(key));
    let cred_repo = Arc::new(MockCredentialRepository { credential: None });
    let manager = ProviderManager::new(repo)
        .with_credential_repo(cred_repo)
        .with_cipher(cipher);

    let result = manager.build_provider_with_model(record, "gpt-4").await;
    assert!(result.is_err(), "should fail without stored credentials");
    let err = result.err().expect("already checked");
    assert!(
        err.to_string().contains("credential") || err.to_string().contains("No stored"),
        "error should mention credentials: {err}"
    );
}

#[tokio::test]
async fn account_token_source_still_works_for_desktop_regression() {
    // The old account_token_source metadata key should still be recognized.
    // It should fail gracefully without account_repo, preserving desktop behavior.
    let mut record = make_static_record();
    record.meta_data.insert("account_token_source".to_string(), "true".to_string());

    let repo = Arc::new(MockProviderRepository {
        record: record.clone(),
    });
    let manager = ProviderManager::new(repo);

    let result = manager.build_provider_with_model(record, "gpt-4").await;
    assert!(result.is_err(), "should fail without account repo");
    let err = result.err().expect("already checked");
    assert!(
        err.to_string().contains("AccountRepository"),
        "error should mention AccountRepository for desktop regression: {err}"
    );
}
