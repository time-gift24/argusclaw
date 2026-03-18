//! LLM provider types and repository trait.
//!
//! This module re-exports types from `argus_protocol::llm` for backward compatibility
//! and the repository trait from `argus_repository`.

// Transitional re-exports
#![allow(unused_imports)]

// Re-export all LLM types from argus-protocol for backward compatibility
pub use argus_protocol::llm::{
    LlmProviderId, LlmProviderKind, LlmProviderRecord, ProviderSecretStatus, ProviderTestResult,
    ProviderTestStatus, SecretString,
};

// Re-export repository trait from argus_repository
pub use argus_repository::LlmProviderRepository;

#[cfg(test)]
mod multi_model_tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn llm_provider_record_has_models_and_default_model() {
        let record = LlmProviderRecord {
            id: LlmProviderId::new(1),
            kind: LlmProviderKind::OpenAiCompatible,
            display_name: "Test".to_string(),
            base_url: "https://api.example.com/v1".to_string(),
            api_key: SecretString::new("sk-test"),
            models: vec!["gpt-4.1".to_string(), "gpt-4.1-mini".to_string()],
            default_model: "gpt-4.1".to_string(),
            is_default: true,
            extra_headers: HashMap::new(),
            secret_status: ProviderSecretStatus::Ready,
        };

        assert_eq!(record.models.len(), 2);
        assert_eq!(record.default_model, "gpt-4.1");
    }
}

#[cfg(test)]
mod tests {
    use chrono::{TimeZone, Utc};
    use serde_json::json;

    use super::{ProviderTestResult, ProviderTestStatus};

    #[test]
    fn provider_test_result_serializes_with_stable_shape() {
        let result = ProviderTestResult {
            provider_id: "openai".to_string(),
            model: "gpt-4.1".to_string(),
            base_url: "https://api.example.com/v1".to_string(),
            checked_at: Utc
                .with_ymd_and_hms(2026, 3, 16, 12, 0, 0)
                .single()
                .expect("timestamp should be valid"),
            latency_ms: 42,
            status: ProviderTestStatus::ModelNotAvailable,
            message: "Model gpt-4.1 not available on provider openai-compatible".to_string(),
        };

        let serialized = serde_json::to_value(result).expect("result should serialize");

        assert_eq!(
            serialized,
            json!({
                "provider_id": "openai",
                "model": "gpt-4.1",
                "base_url": "https://api.example.com/v1",
                "checked_at": "2026-03-16T12:00:00Z",
                "latency_ms": 42,
                "status": "model_not_available",
                "message": "Model gpt-4.1 not available on provider openai-compatible",
            })
        );
    }
}
