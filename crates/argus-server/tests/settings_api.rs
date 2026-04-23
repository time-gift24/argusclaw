mod support;

use std::collections::HashMap;

use axum::http::StatusCode;

use argus_protocol::{
    LlmProviderId, LlmProviderKind, LlmProviderRecord, ProviderSecretStatus, SecretString,
};
use argus_server::response::MutationResponse;
use argus_server::routes::settings::{SettingsResponse, UpdateSettingsRequest};

#[tokio::test]
async fn settings_get_and_put_round_trip() {
    let ctx = support::TestContext::new().await;
    let provider_id = ctx
        .core
        .upsert_provider(LlmProviderRecord {
            id: LlmProviderId::new(0),
            kind: LlmProviderKind::OpenAiCompatible,
            display_name: "Workspace Provider".to_string(),
            base_url: "https://example.invalid/v1".to_string(),
            api_key: SecretString::new("sk-settings"),
            models: vec!["workspace-model".to_string()],
            model_config: HashMap::new(),
            default_model: "workspace-model".to_string(),
            is_default: false,
            extra_headers: HashMap::new(),
            secret_status: ProviderSecretStatus::Ready,
            meta_data: HashMap::new(),
        })
        .await
        .expect("provider should upsert")
        .into_inner();

    let response = ctx.get("/api/v1/settings").await;
    assert_eq!(response.status(), StatusCode::OK);
    let initial: SettingsResponse = support::json_body(response).await;
    assert_ne!(initial.default_provider_id, provider_id);

    let response = ctx
        .put_json(
            "/api/v1/settings",
            &UpdateSettingsRequest {
                instance_name: "Workspace Admin".to_string(),
                default_provider_id: provider_id,
            },
        )
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    let updated: MutationResponse<SettingsResponse> = support::json_body(response).await;
    assert_eq!(updated.item.instance_name, "Workspace Admin");
    assert_eq!(updated.item.default_provider_id, provider_id);
    assert_eq!(updated.item.default_provider_name, "Workspace Provider");
}
