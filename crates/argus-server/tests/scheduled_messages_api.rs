mod support;

use std::collections::HashMap;

use argus_protocol::llm::ModelConfig;
use argus_protocol::{
    AgentId, AgentRecord, LlmProviderKind, LlmProviderRecordJson, ProviderSecretStatus,
    ThinkingConfig,
};
use argus_server::response::MutationResponse;
use axum::http::StatusCode;
use serde_json::json;

#[tokio::test]
async fn scheduled_message_routes_create_list_and_pause() {
    let ctx = support::TestContext::new().await;
    let (session_id, thread_id) = create_chat_session_with_thread(&ctx).await;

    let create = ctx
        .post_json(
            "/api/v1/scheduled-messages",
            &json!({
                "session_id": session_id,
                "thread_id": thread_id,
                "name": "Daily check",
                "prompt": "Run the daily check",
                "cron_expr": "0 9 * * *",
                "timezone": "Asia/Shanghai"
            }),
        )
        .await;
    assert_eq!(create.status(), StatusCode::CREATED);
    let created: MutationResponse<serde_json::Value> = support::json_body(create).await;
    let id = created.item["id"]
        .as_str()
        .expect("scheduled message id should be a string");

    let list = ctx.get("/api/v1/scheduled-messages").await;
    assert_eq!(list.status(), StatusCode::OK);
    let body: Vec<serde_json::Value> = support::json_body(list).await;
    assert_eq!(body.len(), 1);
    assert_eq!(body[0]["id"], id);
    assert_eq!(body[0]["session_id"], session_id);
    assert_eq!(body[0]["thread_id"], thread_id);

    let pause = ctx
        .post_json(
            &format!("/api/v1/scheduled-messages/{id}/pause"),
            &json!({}),
        )
        .await;
    assert_eq!(pause.status(), StatusCode::OK);
    let paused: MutationResponse<serde_json::Value> = support::json_body(pause).await;
    assert_eq!(paused.item["status"], "paused");
}

#[tokio::test]
async fn scheduled_message_trigger_returns_completed_one_shot() {
    let ctx = support::TestContext::new().await;
    let (session_id, thread_id) = create_chat_session_with_thread(&ctx).await;

    let create = ctx
        .post_json(
            "/api/v1/scheduled-messages",
            &json!({
                "session_id": session_id,
                "thread_id": thread_id,
                "name": "One shot",
                "prompt": "Run once",
                "scheduled_at": "2099-05-08T01:00:00Z"
            }),
        )
        .await;
    assert_eq!(create.status(), StatusCode::CREATED);
    let created: MutationResponse<serde_json::Value> = support::json_body(create).await;
    let id = created.item["id"]
        .as_str()
        .expect("scheduled message id should be a string");

    let trigger = ctx
        .post_json(
            &format!("/api/v1/scheduled-messages/{id}/trigger"),
            &json!({}),
        )
        .await;
    assert_eq!(trigger.status(), StatusCode::OK);
    let triggered: MutationResponse<serde_json::Value> = support::json_body(trigger).await;
    assert_eq!(triggered.item["id"], id);
    assert_eq!(triggered.item["status"], "succeeded");
}

async fn create_chat_session_with_thread(ctx: &support::TestContext) -> (String, String) {
    let provider = create_test_provider(ctx).await;
    let template = first_template(ctx).await;
    let response = ctx
        .post_json(
            "/api/v1/chat/sessions/with-thread",
            &json!({
                "name": "Scheduled Messages",
                "template_id": template.id,
                "provider_id": provider.id,
                "model": "alpha"
            }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let created: MutationResponse<serde_json::Value> = support::json_body(response).await;
    (
        created.item["session_id"].as_str().unwrap().to_string(),
        created.item["thread_id"].as_str().unwrap().to_string(),
    )
}

async fn create_test_provider(ctx: &support::TestContext) -> LlmProviderRecordJson {
    let response = ctx
        .post_json(
            "/api/v1/providers",
            &LlmProviderRecordJson {
                id: 0,
                kind: LlmProviderKind::OpenAiCompatible,
                display_name: "Scheduled Messages API Provider".to_string(),
                base_url: "https://example.invalid/v1".to_string(),
                api_key: "sk-scheduled-messages-api".to_string(),
                models: vec!["alpha".to_string()],
                model_config: HashMap::from([(
                    "alpha".to_string(),
                    ModelConfig {
                        max_context_window: 65_536,
                    },
                )]),
                default_model: "alpha".to_string(),
                is_default: true,
                extra_headers: HashMap::new(),
                secret_status: ProviderSecretStatus::Ready,
                meta_data: HashMap::new(),
            },
        )
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let created: MutationResponse<LlmProviderRecordJson> = support::json_body(response).await;
    created.item
}

async fn first_template(ctx: &support::TestContext) -> AgentRecord {
    let response = ctx.get("/api/v1/agents/templates").await;
    assert_eq!(response.status(), StatusCode::OK);
    let templates: Vec<AgentRecord> = support::json_body(response).await;
    if let Some(template) = templates.into_iter().next() {
        return template;
    }

    let response = ctx
        .post_json(
            "/api/v1/agents/templates",
            &AgentRecord {
                id: AgentId::new(0),
                display_name: "Scheduled Messages Test Agent".to_string(),
                description: "created by scheduled messages API test".to_string(),
                version: "1.0.0".to_string(),
                provider_id: None,
                model_id: Some("alpha".to_string()),
                system_prompt: "You are a test chat agent.".to_string(),
                tool_names: vec![],
                subagent_names: vec![],
                max_tokens: None,
                temperature: None,
                thinking_config: Some(ThinkingConfig::enabled()),
            },
        )
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let created: MutationResponse<AgentRecord> = support::json_body(response).await;
    created.item
}
