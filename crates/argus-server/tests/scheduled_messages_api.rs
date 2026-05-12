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
    let provider = create_test_provider(&ctx).await;
    let template = first_template(&ctx).await;

    let create = ctx
        .post_json(
            "/api/v1/scheduled-messages",
            &json!({
                "name": "Daily check",
                "prompt": "Run the daily check",
                "template_id": template.id,
                "provider_id": provider.id,
                "model": "alpha",
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
    assert_eq!(body[0]["template_id"], template.id.inner());
    assert_eq!(body[0]["provider_id"], provider.id);
    assert_eq!(body[0]["model"], "alpha");
    assert!(body[0]["last_session_id"].is_null());
    assert!(body[0]["last_thread_id"].is_null());

    let update = ctx
        .put_json(
            &format!("/api/v1/scheduled-messages/{id}"),
            &json!({
                "name": "Daily check updated",
                "prompt": "Run the updated check",
                "template_id": template.id,
                "provider_id": provider.id,
                "model": "alpha",
                "cron_expr": "30 8 * * *",
                "timezone": "UTC"
            }),
        )
        .await;
    assert_eq!(update.status(), StatusCode::OK);
    let updated: MutationResponse<serde_json::Value> = support::json_body(update).await;
    assert_eq!(updated.item["id"], id);
    assert_eq!(updated.item["name"], "Daily check updated");
    assert_eq!(updated.item["prompt"], "Run the updated check");
    assert_eq!(updated.item["cron_expr"], "30 8 * * *");
    assert_eq!(updated.item["timezone"], "UTC");

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
    let provider = create_test_provider(&ctx).await;
    let template = first_template(&ctx).await;

    let create = ctx
        .post_json(
            "/api/v1/scheduled-messages",
            &json!({
                "name": "One shot",
                "prompt": "Run once",
                "template_id": template.id,
                "provider_id": provider.id,
                "model": "alpha",
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
    let session_id = triggered.item["last_session_id"]
        .as_str()
        .expect("trigger should create a session");
    let thread_id = triggered.item["last_thread_id"]
        .as_str()
        .expect("trigger should create a thread");

    let thread = ctx
        .get(&format!(
            "/api/v1/chat/sessions/{session_id}/threads/{thread_id}"
        ))
        .await;
    assert_eq!(thread.status(), StatusCode::OK);
}

#[tokio::test]
async fn recurring_scheduled_message_creates_a_fresh_thread_each_trigger() {
    let ctx = support::TestContext::new().await;
    let provider = create_test_provider(&ctx).await;
    let template = first_template(&ctx).await;

    let create = ctx
        .post_json(
            "/api/v1/scheduled-messages",
            &json!({
                "name": "Recurring check",
                "prompt": "Run recurring",
                "template_id": template.id,
                "provider_id": provider.id,
                "model": "alpha",
                "cron_expr": "0 9 * * *",
                "timezone": "UTC"
            }),
        )
        .await;
    assert_eq!(create.status(), StatusCode::CREATED);
    let created: MutationResponse<serde_json::Value> = support::json_body(create).await;
    let id = created.item["id"].as_str().unwrap();

    let first = ctx
        .post_json(
            &format!("/api/v1/scheduled-messages/{id}/trigger"),
            &json!({}),
        )
        .await;
    assert_eq!(first.status(), StatusCode::OK);
    let first_body: MutationResponse<serde_json::Value> = support::json_body(first).await;
    let first_thread = first_body.item["last_thread_id"]
        .as_str()
        .unwrap()
        .to_string();

    let second = ctx
        .post_json(
            &format!("/api/v1/scheduled-messages/{id}/trigger"),
            &json!({}),
        )
        .await;
    assert_eq!(second.status(), StatusCode::OK);
    let second_body: MutationResponse<serde_json::Value> = support::json_body(second).await;
    let second_thread = second_body.item["last_thread_id"].as_str().unwrap();

    assert_ne!(first_thread, second_thread);
    assert_eq!(second_body.item["status"], "pending");
}

#[tokio::test]
async fn scheduled_messages_are_owned_by_the_creating_user() {
    let ctx = support::TestContext::new().await;
    let provider = create_test_provider(&ctx).await;
    let template = first_template(&ctx).await;

    let create = ctx
        .post_json(
            "/api/v1/scheduled-messages",
            &json!({
                "name": "Private schedule",
                "prompt": "Only owner can run",
                "template_id": template.id,
                "provider_id": provider.id,
                "model": "alpha",
                "scheduled_at": "2099-05-08T01:00:00Z"
            }),
        )
        .await;
    assert_eq!(create.status(), StatusCode::CREATED);
    let created: MutationResponse<serde_json::Value> = support::json_body(create).await;
    let id = created.item["id"].as_str().unwrap();

    let list_for_other = ctx
        .get_as("/api/v1/scheduled-messages", support::ALT_TEST_USER_ID)
        .await;
    assert_eq!(list_for_other.status(), StatusCode::OK);
    let other_body: Vec<serde_json::Value> = support::json_body(list_for_other).await;
    assert!(other_body.is_empty());

    let trigger_for_other = ctx
        .post_json_as(
            &format!("/api/v1/scheduled-messages/{id}/trigger"),
            &json!({}),
            support::ALT_TEST_USER_ID,
        )
        .await;
    assert_eq!(trigger_for_other.status(), StatusCode::NOT_FOUND);
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
