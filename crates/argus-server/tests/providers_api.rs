mod support;

use std::collections::HashMap;

use axum::http::StatusCode;
use serde_json::json;

use argus_protocol::llm::ModelConfig;
use argus_protocol::{LlmProviderKind, LlmProviderRecordJson, ProviderSecretStatus};
use argus_server::response::MutationResponse;

#[tokio::test]
async fn provider_routes_create_list_and_update() {
    let ctx = support::TestContext::new().await;

    let initial_response = ctx.get("/api/v1/providers").await;
    assert_eq!(initial_response.status(), StatusCode::OK);
    let initial: Vec<LlmProviderRecordJson> = support::json_body(initial_response).await;
    let initial_len = initial.len();

    let create_response = ctx
        .post_json(
            "/api/v1/providers",
            &LlmProviderRecordJson {
                id: 99,
                kind: LlmProviderKind::OpenAiCompatible,
                display_name: "HTTP Provider".to_string(),
                base_url: "https://example.invalid/v1".to_string(),
                api_key: "sk-provider".to_string(),
                models: vec!["alpha".to_string()],
                model_config: HashMap::from([(
                    "alpha".to_string(),
                    ModelConfig {
                        max_context_window: 65_536,
                    },
                )]),
                default_model: "alpha".to_string(),
                is_default: false,
                extra_headers: HashMap::new(),
                secret_status: ProviderSecretStatus::Ready,
                meta_data: HashMap::new(),
            },
        )
        .await;

    assert_eq!(create_response.status(), StatusCode::CREATED);
    let created: MutationResponse<LlmProviderRecordJson> =
        support::json_body(create_response).await;
    assert_ne!(created.item.id, 99);
    assert_eq!(created.item.display_name, "HTTP Provider");

    let update_response = ctx
        .patch_json(
            &format!("/api/v1/providers/{}", created.item.id),
            &LlmProviderRecordJson {
                display_name: "HTTP Provider Updated".to_string(),
                ..created.item.clone()
            },
        )
        .await;

    assert_eq!(update_response.status(), StatusCode::OK);
    let updated: MutationResponse<LlmProviderRecordJson> =
        support::json_body(update_response).await;
    assert_eq!(updated.item.id, created.item.id);
    assert_eq!(updated.item.display_name, "HTTP Provider Updated");
    assert_eq!(updated.item.api_key, "sk-provider");

    let preserved_secret_response = ctx
        .patch_json(
            &format!("/api/v1/providers/{}", created.item.id),
            &json!({
                "id": created.item.id,
                "kind": created.item.kind,
                "display_name": "HTTP Provider Preserved Secret",
                "base_url": created.item.base_url,
                "api_key": null,
                "models": created.item.models,
                "model_config": created.item.model_config,
                "default_model": created.item.default_model,
                "is_default": created.item.is_default,
                "extra_headers": created.item.extra_headers,
                "secret_status": created.item.secret_status,
                "meta_data": created.item.meta_data,
            }),
        )
        .await;

    assert_eq!(preserved_secret_response.status(), StatusCode::OK);
    let preserved_secret: MutationResponse<LlmProviderRecordJson> =
        support::json_body(preserved_secret_response).await;
    assert_eq!(preserved_secret.item.api_key, "sk-provider");
    assert_eq!(
        preserved_secret.item.display_name,
        "HTTP Provider Preserved Secret"
    );

    let final_response = ctx.get("/api/v1/providers").await;
    assert_eq!(final_response.status(), StatusCode::OK);
    let final_body: Vec<LlmProviderRecordJson> = support::json_body(final_response).await;
    assert_eq!(final_body.len(), initial_len + 1);
    assert!(
        final_body
            .iter()
            .any(|provider| provider.display_name == "HTTP Provider Preserved Secret")
    );
}
