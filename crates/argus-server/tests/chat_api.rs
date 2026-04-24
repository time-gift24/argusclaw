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
async fn chat_session_route_materializes_session_and_thread_like_desktop() {
    let ctx = support::TestContext::new().await;
    let provider = create_test_provider(&ctx).await;
    let template = first_template(&ctx).await;

    let response = ctx
        .post_json(
            "/api/v1/chat/sessions/with-thread",
            &json!({
                "template_id": template.id,
                "provider_id": provider.id,
                "model": "alpha"
            }),
        )
        .await;

    assert_eq!(response.status(), StatusCode::CREATED);
    let created: MutationResponse<serde_json::Value> = support::json_body(response).await;
    assert_eq!(created.item["template_id"], template.id.inner());
    assert_eq!(created.item["effective_provider_id"], provider.id);
    assert_eq!(created.item["effective_model"], "alpha");
    assert!(created.item["session_key"].as_str().is_some_and(|value| {
        value.contains(&template.id.inner().to_string()) && value.contains(&provider.id.to_string())
    }));

    let session_id = created.item["session_id"]
        .as_str()
        .expect("session_id should be a string");
    let thread_id = created.item["thread_id"]
        .as_str()
        .expect("thread_id should be a string");

    let threads_response = ctx
        .get(&format!("/api/v1/chat/sessions/{session_id}/threads"))
        .await;
    assert_eq!(threads_response.status(), StatusCode::OK);
    let threads: Vec<ThreadSummary> = support::json_body(threads_response).await;
    assert!(
        threads
            .iter()
            .any(|thread| thread.id.to_string() == thread_id)
    );
}

#[tokio::test]
async fn chat_thread_events_route_opens_stream_for_materialized_thread() {
    let ctx = support::TestContext::new().await;
    let provider = create_test_provider(&ctx).await;
    let template = first_template(&ctx).await;

    let create_response = ctx
        .post_json(
            "/api/v1/chat/sessions/with-thread",
            &json!({
                "template_id": template.id,
                "provider_id": provider.id,
                "model": "alpha"
            }),
        )
        .await;
    let created: MutationResponse<serde_json::Value> = support::json_body(create_response).await;
    let session_id = created.item["session_id"]
        .as_str()
        .expect("session_id should be a string");
    let thread_id = created.item["thread_id"]
        .as_str()
        .expect("thread_id should be a string");

    let response = ctx
        .get(&format!(
            "/api/v1/chat/sessions/{session_id}/threads/{thread_id}/events"
        ))
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    let content_type = response
        .headers()
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|value| value.to_str().ok())
        .expect("event stream should set content-type");
    assert!(content_type.starts_with("text/event-stream"));
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

    assert_eq!(messages_response.status(), StatusCode::NOT_FOUND);
    let body: serde_json::Value = support::json_body(messages_response).await;
    assert_eq!(body["error"]["code"], "not_found");
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

#[tokio::test]
async fn chat_routes_rename_session_and_thread() {
    let ctx = support::TestContext::new().await;
    let provider = create_test_provider(&ctx).await;
    let template = first_template(&ctx).await;
    let session = create_test_session(&ctx, "Before Rename").await;
    let thread = create_test_thread(&ctx, &session, &template, &provider).await;

    let rename_session_response = ctx
        .patch_json(
            &format!("/api/v1/chat/sessions/{}", session.id),
            &json!({ "name": "After Rename" }),
        )
        .await;
    assert_eq!(rename_session_response.status(), StatusCode::OK);
    let renamed_session: MutationResponse<SessionSummary> =
        support::json_body(rename_session_response).await;
    assert_eq!(renamed_session.item.name, "After Rename");

    let rename_thread_response = ctx
        .patch_json(
            &format!("/api/v1/chat/sessions/{}/threads/{}", session.id, thread.id),
            &json!({ "title": "Renamed Thread" }),
        )
        .await;
    assert_eq!(rename_thread_response.status(), StatusCode::OK);
    let renamed_thread: MutationResponse<ThreadSummary> =
        support::json_body(rename_thread_response).await;
    assert_eq!(renamed_thread.item.title.as_deref(), Some("Renamed Thread"));
}

#[tokio::test]
async fn chat_routes_return_thread_snapshot_and_binding() {
    let ctx = support::TestContext::new().await;
    let provider = create_test_provider(&ctx).await;
    let template = first_template(&ctx).await;
    let session = create_test_session(&ctx, "Snapshot Case").await;
    let thread = create_test_thread(&ctx, &session, &template, &provider).await;

    let snapshot_response = ctx
        .get(&format!(
            "/api/v1/chat/sessions/{}/threads/{}",
            session.id, thread.id
        ))
        .await;
    assert_eq!(snapshot_response.status(), StatusCode::OK);
    let snapshot: serde_json::Value = support::json_body(snapshot_response).await;
    assert_eq!(snapshot["session_id"], session.id.to_string());
    assert_eq!(snapshot["thread_id"], thread.id.to_string());
    assert_eq!(snapshot["turn_count"], 0);
    assert_eq!(snapshot["token_count"], 0);
    assert_eq!(snapshot["plan_item_count"], 0);
    assert_eq!(snapshot["messages"].as_array().map(Vec::len), Some(0));

    let update_model_response = ctx
        .patch_json(
            &format!(
                "/api/v1/chat/sessions/{}/threads/{}/model",
                session.id, thread.id
            ),
            &json!({
                "provider_id": provider.id,
                "model": "alpha"
            }),
        )
        .await;
    assert_eq!(update_model_response.status(), StatusCode::OK);
    let model_binding: MutationResponse<serde_json::Value> =
        support::json_body(update_model_response).await;
    assert_eq!(model_binding.item["session_id"], session.id.to_string());
    assert_eq!(model_binding.item["thread_id"], thread.id.to_string());
    assert_eq!(model_binding.item["template_id"], template.id.inner());
    assert_eq!(model_binding.item["effective_provider_id"], provider.id);
    assert_eq!(model_binding.item["effective_model"], "alpha");

    let activate_response = ctx
        .post_empty(&format!(
            "/api/v1/chat/sessions/{}/threads/{}/activate",
            session.id, thread.id
        ))
        .await;
    assert_eq!(activate_response.status(), StatusCode::OK);
    let activation: MutationResponse<serde_json::Value> =
        support::json_body(activate_response).await;
    assert_eq!(activation.item["session_id"], session.id.to_string());
    assert_eq!(activation.item["thread_id"], thread.id.to_string());
    assert_eq!(activation.item["template_id"], template.id.inner());
    assert_eq!(activation.item["effective_provider_id"], provider.id);
    assert_eq!(activation.item["effective_model"], "alpha");
}

#[tokio::test]
async fn chat_routes_use_structured_client_errors() {
    let ctx = support::TestContext::new().await;

    let invalid_id_response = ctx
        .patch_json(
            "/api/v1/chat/sessions/not-a-uuid",
            &json!({ "name": "Ignored" }),
        )
        .await;
    assert_eq!(invalid_id_response.status(), StatusCode::BAD_REQUEST);
    let invalid_body: serde_json::Value = support::json_body(invalid_id_response).await;
    assert_eq!(invalid_body["error"]["code"], "bad_request");

    let unknown_session_response = ctx
        .patch_json(
            &format!("/api/v1/chat/sessions/{}", argus_protocol::SessionId::new()),
            &json!({ "name": "Missing" }),
        )
        .await;
    assert_eq!(unknown_session_response.status(), StatusCode::NOT_FOUND);
    let unknown_body: serde_json::Value = support::json_body(unknown_session_response).await;
    assert_eq!(unknown_body["error"]["code"], "not_found");
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

async fn create_test_thread(
    ctx: &support::TestContext,
    session: &SessionSummary,
    template: &AgentRecord,
    provider: &LlmProviderRecordJson,
) -> ThreadSummary {
    let response = ctx
        .post_json(
            &format!("/api/v1/chat/sessions/{}/threads", session.id),
            &json!({
                "template_id": template.id,
                "provider_id": provider.id,
                "model": "alpha"
            }),
        )
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let created: MutationResponse<ThreadSummary> = support::json_body(response).await;
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
