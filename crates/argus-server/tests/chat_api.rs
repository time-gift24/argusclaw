mod support;

use std::collections::HashMap;

use argus_protocol::ThreadId;
use argus_protocol::llm::ModelConfig;
use argus_protocol::{AgentRecord, LlmProviderKind, LlmProviderRecordJson, ProviderSecretStatus};
use argus_server::response::MutationResponse;
use argus_session::{SessionSummary, ThreadSummary};
use axum::http::StatusCode;
use serde_json::json;

#[tokio::test]
async fn chat_session_routes_create_list_and_show_empty_threads() {
    let ctx = support::TestContext::new().await;

    let create_response = ctx
        .post_json("/api/v1/chat/sessions", &json!({ "name": "Web Chat" }))
        .await;

    assert_eq!(create_response.status(), StatusCode::CREATED);
    let created: MutationResponse<SessionSummary> = support::json_body(create_response).await;
    assert_eq!(created.item.name, "Web Chat");
    assert_eq!(created.item.thread_count, 0);

    let list_response = ctx.get("/api/v1/chat/sessions").await;
    assert_eq!(list_response.status(), StatusCode::OK);
    let sessions: Vec<SessionSummary> = support::json_body(list_response).await;
    assert!(
        sessions
            .iter()
            .any(|session| session.id == created.item.id && session.name == "Web Chat")
    );

    let threads_response = ctx
        .get(&format!(
            "/api/v1/chat/sessions/{}/threads",
            created.item.id
        ))
        .await;
    assert_eq!(threads_response.status(), StatusCode::OK);
    let threads: Vec<ThreadSummary> = support::json_body(threads_response).await;
    assert!(threads.is_empty());
}

#[tokio::test]
async fn chat_messages_route_errors_for_unknown_thread() {
    let ctx = support::TestContext::new().await;
    let create_response = ctx
        .post_json(
            "/api/v1/chat/sessions",
            &json!({ "name": "Missing Thread Case" }),
        )
        .await;
    let created: MutationResponse<SessionSummary> = support::json_body(create_response).await;
    let unknown_thread_id = ThreadId::new();

    let messages_response = ctx
        .get(&format!(
            "/api/v1/chat/sessions/{}/threads/{unknown_thread_id}/messages",
            created.item.id
        ))
        .await;

    assert_eq!(
        messages_response.status(),
        StatusCode::INTERNAL_SERVER_ERROR
    );
    let body: serde_json::Value = support::json_body(messages_response).await;
    assert_eq!(body["error"]["code"], "internal_error");
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("error message should be a string")
            .contains("Thread not found")
    );
}

#[tokio::test]
async fn chat_thread_routes_create_and_delete_thread() {
    let ctx = support::TestContext::new().await;
    let provider = create_test_provider(&ctx).await;
    let template = first_template(&ctx).await;
    let session = create_test_session(&ctx, "Thread Route Case").await;

    let create_thread_response = ctx
        .post_json(
            &format!("/api/v1/chat/sessions/{}/threads", session.id),
            &json!({
                "template_id": template.id,
                "provider_id": provider.id,
                "model": "alpha"
            }),
        )
        .await;

    assert_eq!(create_thread_response.status(), StatusCode::CREATED);
    let created_thread: MutationResponse<ThreadSummary> =
        support::json_body(create_thread_response).await;

    let list_threads_response = ctx
        .get(&format!("/api/v1/chat/sessions/{}/threads", session.id))
        .await;
    assert_eq!(list_threads_response.status(), StatusCode::OK);
    let threads: Vec<ThreadSummary> = support::json_body(list_threads_response).await;
    assert!(
        threads
            .iter()
            .any(|thread| thread.id == created_thread.item.id)
    );

    let delete_response = ctx
        .delete(&format!(
            "/api/v1/chat/sessions/{}/threads/{}",
            session.id, created_thread.item.id
        ))
        .await;
    assert_eq!(delete_response.status(), StatusCode::OK);

    let final_threads_response = ctx
        .get(&format!("/api/v1/chat/sessions/{}/threads", session.id))
        .await;
    let final_threads: Vec<ThreadSummary> = support::json_body(final_threads_response).await;
    assert!(
        final_threads
            .iter()
            .all(|thread| thread.id != created_thread.item.id)
    );
}

async fn create_test_session(ctx: &support::TestContext, name: &str) -> SessionSummary {
    let response = ctx
        .post_json("/api/v1/chat/sessions", &json!({ "name": name }))
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let created: MutationResponse<SessionSummary> = support::json_body(response).await;
    created.item
}

async fn create_test_provider(ctx: &support::TestContext) -> LlmProviderRecordJson {
    let response = ctx
        .post_json(
            "/api/v1/providers",
            &LlmProviderRecordJson {
                id: 0,
                kind: LlmProviderKind::OpenAiCompatible,
                display_name: "Chat API Provider".to_string(),
                base_url: "https://example.invalid/v1".to_string(),
                api_key: "sk-chat-api".to_string(),
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
    templates
        .into_iter()
        .next()
        .expect("test server should seed at least one template")
}
