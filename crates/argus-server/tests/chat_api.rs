mod support;

use std::collections::HashMap;
use std::sync::Arc;

use argus_protocol::llm::{ChatMessage, ModelConfig};
use argus_protocol::{
    AgentId, AgentRecord, LlmProviderId, LlmProviderKind, LlmProviderRecordJson,
    ProviderSecretStatus, SessionId, ThinkingConfig, ThreadId,
};
use argus_repository::traits::{JobRepository, ThreadRepository};
use argus_repository::types::{
    JobId, JobRecord, JobResult, JobStatus, JobType, MessageRecord, ThreadRecord,
};
use argus_repository::{ArgusSqlite, migrate};
use argus_server::app_state::AppState;
use argus_server::response::MutationResponse;
use argus_server::server_core::ServerCore;
use argus_session::{SessionSummary, ThreadSummary};
use axum::Router;
use axum::body::Body;
use axum::http::{Method, StatusCode};
use serde::Serialize;
use serde_json::json;
use tower::util::ServiceExt;

#[tokio::test]
async fn chat_routes_fail_closed_without_trusted_user_header() {
    let ctx = support::TestContext::new().await;

    let response = ctx
        .get_without_default_user_header("/api/v1/chat/sessions")
        .await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = support::json_body(response).await;
    assert_eq!(body["error"]["code"], "unauthorized");
    assert!(
        body["error"]["message"]
            .as_str()
            .expect("error message should be a string")
            .contains("x-argus-user-id")
    );
}

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
async fn chat_session_routes_fail_closed_without_user_header() {
    let ctx = support::TestContext::new().await;

    let response = ctx.get_without_chat_user("/api/v1/chat/sessions").await;

    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let body: serde_json::Value = support::json_body(response).await;
    assert_eq!(body["error"]["code"], "unauthorized");
}

#[tokio::test]
async fn agent_run_routes_create_and_query_run_status() {
    let ctx = support::TestContext::new().await;
    let _provider = create_test_provider(&ctx).await;
    let template = first_template(&ctx).await;

    let create_response = ctx
        .post_json(
            "/api/v1/agents/runs",
            &json!({
                "agent_id": template.id,
                "prompt": "Summarize the repository boundary"
            }),
        )
        .await;

    assert_eq!(create_response.status(), StatusCode::CREATED);
    let created: serde_json::Value = support::json_body(create_response).await;
    assert_eq!(created["data"]["agent_id"], template.id.inner());
    assert_eq!(created["data"]["status"], "queued");
    assert!(created["data"]["created_at"].as_str().is_some());
    assert!(created["data"]["updated_at"].as_str().is_some());

    let run_id = created["data"]["run_id"]
        .as_str()
        .expect("run_id should be a string");
    let get_response = ctx.get(&format!("/api/v1/agents/runs/{run_id}")).await;
    assert_eq!(get_response.status(), StatusCode::OK);

    let run: serde_json::Value = support::json_body(get_response).await;
    assert_eq!(run["run_id"], run_id);
    assert_eq!(run["agent_id"], template.id.inner());
    assert_eq!(run["prompt"], "Summarize the repository boundary");
    assert!(matches!(
        run["status"].as_str(),
        Some("queued" | "running" | "completed" | "failed")
    ));
}

#[tokio::test]
async fn agent_run_routes_reject_empty_prompt_and_unknown_run() {
    let ctx = support::TestContext::new().await;
    let template = first_template(&ctx).await;

    let bad_request = ctx
        .post_json(
            "/api/v1/agents/runs",
            &json!({
                "agent_id": template.id,
                "prompt": "   "
            }),
        )
        .await;
    assert_eq!(bad_request.status(), StatusCode::BAD_REQUEST);
    let bad_body: serde_json::Value = support::json_body(bad_request).await;
    assert_eq!(bad_body["error"]["code"], "bad_request");

    let unknown_response = ctx
        .get(&format!("/api/v1/agents/runs/{}", ThreadId::new()))
        .await;
    assert_eq!(unknown_response.status(), StatusCode::NOT_FOUND);
    let unknown_body: serde_json::Value = support::json_body(unknown_response).await;
    assert_eq!(unknown_body["error"]["code"], "not_found");
}

#[tokio::test]
async fn agent_run_routes_do_not_treat_chat_thread_ids_as_run_ids() {
    let ctx = support::TestContext::new().await;
    let provider = create_test_provider(&ctx).await;
    let template = first_template(&ctx).await;
    let session = create_test_session(&ctx, "Plain Chat Session").await;
    let thread = create_test_thread(&ctx, &session, &template, &provider).await;

    let response = ctx.get(&format!("/api/v1/agents/runs/{}", thread.id)).await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);
    let body: serde_json::Value = support::json_body(response).await;
    assert_eq!(body["error"]["code"], "not_found");
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
                "name": "Web Draft",
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

    let sessions_response = ctx.get("/api/v1/chat/sessions").await;
    assert_eq!(sessions_response.status(), StatusCode::OK);
    let sessions: Vec<SessionSummary> = support::json_body(sessions_response).await;
    assert!(
        sessions
            .iter()
            .any(|session| session.id.to_string() == session_id && session.name == "Web Draft")
    );

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

    let sessions_response = ctx.get("/api/v1/chat/sessions").await;
    assert_eq!(sessions_response.status(), StatusCode::OK);
    let sessions: Vec<SessionSummary> = support::json_body(sessions_response).await;
    assert!(
        sessions
            .iter()
            .any(|session| session.id.to_string() == session_id && session.name == "Web Chat")
    );

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
async fn chat_routes_fail_closed_without_trusted_user_header_all_methods() {
    let ctx = support::TestContext::new().await;

    for (method, path, body) in [
        (Method::GET, "/api/v1/chat/sessions", None),
        (
            Method::POST,
            "/api/v1/chat/sessions",
            Some(json!({ "name": "No Header" })),
        ),
    ] {
        let response = ctx
            .request_without_trusted_user(method, path, body.as_ref())
            .await;

        assert_eq!(
            response.status(),
            StatusCode::UNAUTHORIZED,
            "{path} must reject missing trusted user context"
        );
    }
}

#[tokio::test]
async fn chat_routes_hide_sessions_and_threads_across_trusted_users() {
    let Some(ctx) = support::TestContext::postgres_if_configured().await else {
        return;
    };
    let provider = create_test_provider(&ctx).await;
    let template = first_template(&ctx).await;

    let create_response = ctx
        .post_json_as(
            "/api/v1/chat/sessions",
            &json!({ "name": "User A Session" }),
            support::DEFAULT_TEST_USER_ID,
        )
        .await;
    assert_eq!(create_response.status(), StatusCode::CREATED);
    let session: MutationResponse<SessionSummary> = support::json_body(create_response).await;

    let thread_response = ctx
        .post_json_as(
            &format!("/api/v1/chat/sessions/{}/threads", session.item.id),
            &json!({
                "template_id": template.id,
                "provider_id": provider.id,
                "model": "alpha"
            }),
            support::DEFAULT_TEST_USER_ID,
        )
        .await;
    assert_eq!(thread_response.status(), StatusCode::CREATED);
    let thread: MutationResponse<ThreadSummary> = support::json_body(thread_response).await;

    let user_b_sessions = ctx
        .get_as("/api/v1/chat/sessions", support::ALT_TEST_USER_ID)
        .await;
    assert_eq!(user_b_sessions.status(), StatusCode::OK);
    let sessions: Vec<SessionSummary> = support::json_body(user_b_sessions).await;
    assert!(
        sessions
            .iter()
            .all(|candidate| candidate.id != session.item.id),
        "user B must not list user A sessions"
    );

    let user_b_threads = ctx
        .get_as(
            &format!("/api/v1/chat/sessions/{}/threads", session.item.id),
            support::ALT_TEST_USER_ID,
        )
        .await;
    assert_eq!(user_b_threads.status(), StatusCode::NOT_FOUND);

    let user_b_snapshot = ctx
        .get_as(
            &format!(
                "/api/v1/chat/sessions/{}/threads/{}",
                session.item.id, thread.item.id
            ),
            support::ALT_TEST_USER_ID,
        )
        .await;
    assert_eq!(user_b_snapshot.status(), StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn chat_session_with_thread_rolls_back_session_when_thread_creation_fails() {
    let ctx = support::TestContext::new().await;
    let initial_sessions_response = ctx.get("/api/v1/chat/sessions").await;
    assert_eq!(initial_sessions_response.status(), StatusCode::OK);
    let initial_sessions: Vec<SessionSummary> = support::json_body(initial_sessions_response).await;

    let response = ctx
        .post_json(
            "/api/v1/chat/sessions/with-thread",
            &json!({
                "name": "Should Roll Back",
                "template_id": 999_999,
                "provider_id": null,
                "model": null
            }),
        )
        .await;

    assert_eq!(response.status(), StatusCode::NOT_FOUND);

    let sessions_response = ctx.get("/api/v1/chat/sessions").await;
    assert_eq!(sessions_response.status(), StatusCode::OK);
    let sessions: Vec<SessionSummary> = support::json_body(sessions_response).await;
    assert_eq!(sessions.len(), initial_sessions.len());
    assert!(
        !sessions
            .iter()
            .any(|session| session.name == "Should Roll Back")
    );
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

#[tokio::test]
async fn chat_job_message_post_route_does_not_exist() {
    let ctx = support::TestContext::new().await;
    let response = ctx
        .post_json(
            "/api/v1/chat/jobs/job-123/messages",
            &serde_json::json!({
                "message": "should not be accepted"
            }),
        )
        .await;

    assert_eq!(response.status(), axum::http::StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn list_thread_jobs_requires_valid_ids() {
    let ctx = support::TestContext::new().await;
    let response = ctx
        .get("/api/v1/chat/sessions/not-a-uuid/threads/not-a-thread/jobs")
        .await;

    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn get_chat_job_requires_non_empty_job_id() {
    let ctx = support::TestContext::new().await;
    let response = ctx.get("/api/v1/chat/jobs/%20").await;

    assert_eq!(response.status(), axum::http::StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn thread_jobs_lists_dispatched_subagents_for_parent_thread() {
    let ctx = SeededChatContext::new().await;
    let provider = create_seeded_provider(&ctx).await;
    let template = first_seeded_template(&ctx).await;
    let session = create_seeded_session(&ctx, "Parent Job List").await;
    let parent_thread = create_seeded_thread(&ctx, &session, &template, &provider).await;
    let child_thread_id = ThreadId::new();
    let job_id = "job-list-child";

    ctx.seed_child_job(
        SeedChildJob {
            job_id,
            parent_session_id: session.id,
            parent_thread_id: parent_thread.id,
            child_thread_id,
            provider_id: provider.id,
            agent_id: template.id,
            name: "Fallback child job name",
            status: JobStatus::Succeeded,
            result: Some(JobResult {
                success: true,
                message: "Child job completed with a useful summary".to_string(),
                token_usage: None,
                agent_id: template.id,
                agent_display_name: "Research Subagent".to_string(),
                agent_description: "Looks things up".to_string(),
            }),
            thread_title: Some("Child investigation".to_string()),
        },
        Vec::new(),
    )
    .await;

    let response = ctx
        .get(&format!(
            "/api/v1/chat/sessions/{}/threads/{}/jobs",
            session.id, parent_thread.id
        ))
        .await;

    assert_eq!(response.status(), StatusCode::OK);
    let jobs: serde_json::Value = support::json_body(response).await;
    let items = jobs.as_array().expect("jobs response should be an array");
    assert_eq!(items.len(), 1);
    assert_eq!(items[0]["job_id"], job_id);
    assert_eq!(items[0]["status"], "succeeded");
    assert_eq!(items[0]["subagent_name"], "Research Subagent");
    assert_eq!(items[0]["bound_thread_id"], child_thread_id.to_string());
}

#[tokio::test]
async fn get_chat_job_returns_readonly_conversation_messages() {
    let ctx = SeededChatContext::new().await;
    let provider = create_seeded_provider(&ctx).await;
    let template = first_seeded_template(&ctx).await;
    let session = create_seeded_session(&ctx, "Job Conversation Parent").await;
    let parent_thread = create_seeded_thread(&ctx, &session, &template, &provider).await;
    let child_thread_id = ThreadId::new();
    let job_id = "job-conversation-bound";

    ctx.seed_child_job(
        SeedChildJob {
            job_id,
            parent_session_id: session.id,
            parent_thread_id: parent_thread.id,
            child_thread_id,
            provider_id: provider.id,
            agent_id: template.id,
            name: "Conversation job",
            status: JobStatus::Running,
            result: None,
            thread_title: Some("Readonly child conversation".to_string()),
        },
        vec![ChatMessage::assistant("persisted child reply")],
    )
    .await;

    let response = ctx.get(&format!("/api/v1/chat/jobs/{job_id}")).await;

    assert_eq!(response.status(), StatusCode::OK);
    let conversation: serde_json::Value = support::json_body(response).await;
    assert_eq!(conversation["job_id"], job_id);
    assert_eq!(conversation["thread_id"], child_thread_id.to_string());
    assert_eq!(
        conversation["parent_thread_id"],
        parent_thread.id.to_string()
    );
    assert_eq!(conversation["messages"].as_array().map(Vec::len), Some(1));
    assert_eq!(conversation["messages"][0]["role"], "assistant");
    assert_eq!(
        conversation["messages"][0]["content"],
        "persisted child reply"
    );
}

#[tokio::test]
async fn get_chat_job_reports_pending_when_job_has_no_thread_binding() {
    let ctx = SeededChatContext::new().await;
    let template = first_seeded_template(&ctx).await;
    let job_id = "job-pending-unbound";

    ctx.seed_job_record(JobRecord {
        id: JobId::new(job_id),
        job_type: JobType::Standalone,
        name: "Unbound pending job".to_string(),
        status: JobStatus::Pending,
        agent_id: template.id,
        context: None,
        prompt: "Wait for execution".to_string(),
        thread_id: None,
        group_id: None,
        depends_on: Vec::new(),
        cron_expr: None,
        scheduled_at: Some("2026-05-11T00:00:00Z".to_string()),
        started_at: None,
        finished_at: None,
        parent_job_id: None,
        result: None,
    })
    .await;

    let response = ctx.get(&format!("/api/v1/chat/jobs/{job_id}")).await;

    assert_eq!(response.status(), StatusCode::OK);
    let conversation: serde_json::Value = support::json_body(response).await;
    assert_eq!(conversation["job_id"], job_id);
    assert_eq!(conversation["status"], "pending");
    assert!(conversation["thread_id"].is_null());
    assert_eq!(conversation["messages"].as_array().map(Vec::len), Some(0));
}

async fn create_test_session(ctx: &support::TestContext, name: &str) -> SessionSummary {
    let response = ctx
        .post_json("/api/v1/chat/sessions", &json!({ "name": name }))
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let created: MutationResponse<SessionSummary> = support::json_body(response).await;
    created.item
}

struct SeededChatContext {
    app: Router,
    repo: Arc<ArgusSqlite>,
}

struct SeedChildJob<'a> {
    job_id: &'a str,
    parent_session_id: SessionId,
    parent_thread_id: ThreadId,
    child_thread_id: ThreadId,
    provider_id: i64,
    agent_id: AgentId,
    name: &'a str,
    status: JobStatus,
    result: Option<JobResult>,
    thread_title: Option<String>,
}

impl SeededChatContext {
    async fn new() -> Self {
        let pool = sqlx::SqlitePool::connect("sqlite::memory:")
            .await
            .expect("sqlite pool should connect for seeded chat tests");
        migrate(&pool)
            .await
            .expect("test migrations should succeed");
        let repo = Arc::new(ArgusSqlite::new(pool.clone()));
        let core = ServerCore::with_pool(pool)
            .await
            .expect("server core should initialize for tests");
        let app = argus_server::router(AppState::new(core));
        Self { app, repo }
    }

    async fn get(&self, path: &str) -> axum::http::Response<Body> {
        self.request(Method::GET, path, Option::<&()>::None).await
    }

    async fn post_json<T>(&self, path: &str, payload: &T) -> axum::http::Response<Body>
    where
        T: Serialize,
    {
        self.request(Method::POST, path, Some(payload)).await
    }

    async fn request<T>(
        &self,
        method: Method,
        path: &str,
        payload: Option<&T>,
    ) -> axum::http::Response<Body>
    where
        T: Serialize,
    {
        let mut request = axum::http::Request::builder()
            .method(method)
            .uri(path)
            .header("x-argus-user-id", support::DEFAULT_TEST_USER_ID);
        let body = match payload {
            Some(payload) => {
                request = request.header("content-type", "application/json");
                Body::from(
                    serde_json::to_vec(payload).expect("request payload should serialize to json"),
                )
            }
            None => Body::empty(),
        };

        self.app
            .clone()
            .oneshot(request.body(body).expect("request should build"))
            .await
            .expect("response should succeed")
    }

    async fn seed_job_record(&self, job: JobRecord) {
        let result = job.result.clone();
        let job_id = job.id.clone();
        JobRepository::create(self.repo.as_ref(), &job)
            .await
            .expect("job record should seed");
        if let Some(result) = result {
            JobRepository::update_result(self.repo.as_ref(), &job_id, &result)
                .await
                .expect("job result should seed");
        }
    }

    async fn seed_child_job(&self, seed: SeedChildJob<'_>, messages: Vec<ChatMessage>) {
        let now = "2026-05-11T00:00:00Z".to_string();
        ThreadRepository::upsert_thread(
            self.repo.as_ref(),
            &ThreadRecord {
                id: seed.child_thread_id,
                provider_id: LlmProviderId::new(seed.provider_id),
                title: seed.thread_title,
                token_count: 17,
                turn_count: messages.len() as u32,
                session_id: None,
                template_id: Some(seed.agent_id),
                model_override: Some("alpha".to_string()),
                created_at: now.clone(),
                updated_at: now.clone(),
            },
        )
        .await
        .expect("child thread should seed");

        for (index, message) in messages.into_iter().enumerate() {
            ThreadRepository::add_message(
                self.repo.as_ref(),
                &MessageRecord {
                    id: None,
                    thread_id: seed.child_thread_id,
                    seq: index as u32 + 1,
                    role: role_name(message.role).to_string(),
                    content: message.content,
                    tool_call_id: message.tool_call_id,
                    tool_name: message.name,
                    tool_calls: message
                        .tool_calls
                        .map(|tool_calls| serde_json::to_string(&tool_calls).unwrap()),
                    created_at: now.clone(),
                },
            )
            .await
            .expect("child message should seed");
        }

        self.seed_job_record(JobRecord {
            id: JobId::new(seed.job_id),
            job_type: JobType::Standalone,
            name: seed.name.to_string(),
            status: seed.status,
            agent_id: seed.agent_id,
            context: None,
            prompt: "Seed child job".to_string(),
            thread_id: Some(seed.child_thread_id),
            group_id: None,
            depends_on: Vec::new(),
            cron_expr: None,
            scheduled_at: Some(now.clone()),
            started_at: Some(now.clone()),
            finished_at: None,
            parent_job_id: None,
            result: seed.result,
        })
        .await;

        persist_child_trace_metadata(
            seed.parent_session_id,
            seed.parent_thread_id,
            seed.child_thread_id,
            seed.job_id,
            seed.agent_id,
        )
        .await;
    }
}

async fn persist_child_trace_metadata(
    parent_session_id: SessionId,
    parent_thread_id: ThreadId,
    child_thread_id: ThreadId,
    job_id: &str,
    agent_id: AgentId,
) {
    let trace_root = dirs::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("~"))
        .join(".arguswing")
        .join("traces");
    let child_dir = trace_root
        .join(parent_session_id.to_string())
        .join(parent_thread_id.to_string())
        .join(child_thread_id.to_string());
    tokio::fs::create_dir_all(&child_dir)
        .await
        .expect("child trace dir should seed");
    let metadata = json!({
        "thread_id": child_thread_id,
        "kind": "Job",
        "root_session_id": null,
        "parent_thread_id": parent_thread_id,
        "job_id": job_id,
        "agent_snapshot": {
            "id": agent_id,
            "display_name": "Seeded Subagent",
            "description": "Seeded from chat API test",
            "version": "1.0.0",
            "provider_id": null,
            "model_id": "alpha",
            "system_prompt": "You are a seeded subagent.",
            "tool_names": [],
            "subagent_names": [],
            "max_tokens": null,
            "temperature": null,
            "thinking_config": null
        }
    });
    tokio::fs::write(
        child_dir.join("thread.json"),
        serde_json::to_vec_pretty(&metadata).unwrap(),
    )
    .await
    .expect("child trace metadata should seed");
}

async fn create_seeded_session(ctx: &SeededChatContext, name: &str) -> SessionSummary {
    let response = ctx
        .post_json("/api/v1/chat/sessions", &json!({ "name": name }))
        .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let created: MutationResponse<SessionSummary> = support::json_body(response).await;
    created.item
}

async fn create_seeded_provider(ctx: &SeededChatContext) -> LlmProviderRecordJson {
    let response = ctx
        .post_json(
            "/api/v1/providers",
            &LlmProviderRecordJson {
                id: 0,
                kind: LlmProviderKind::OpenAiCompatible,
                display_name: "Seeded Chat API Provider".to_string(),
                base_url: "https://example.invalid/v1".to_string(),
                api_key: "sk-seeded-chat-api".to_string(),
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

async fn create_seeded_thread(
    ctx: &SeededChatContext,
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

async fn first_seeded_template(ctx: &SeededChatContext) -> AgentRecord {
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
                display_name: "Seeded Chat Agent".to_string(),
                description: "created by seeded chat API test".to_string(),
                version: "1.0.0".to_string(),
                provider_id: None,
                model_id: Some("alpha".to_string()),
                system_prompt: "You are a seeded chat agent.".to_string(),
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

fn role_name(role: argus_protocol::llm::Role) -> &'static str {
    match role {
        argus_protocol::llm::Role::System => "system",
        argus_protocol::llm::Role::User => "user",
        argus_protocol::llm::Role::Assistant => "assistant",
        argus_protocol::llm::Role::Tool => "tool",
    }
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
    if let Some(template) = templates.into_iter().next() {
        return template;
    }

    let response = ctx
        .post_json(
            "/api/v1/agents/templates",
            &AgentRecord {
                id: AgentId::new(0),
                display_name: "Test Chat Agent".to_string(),
                description: "created by chat API test".to_string(),
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
