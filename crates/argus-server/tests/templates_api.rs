mod support;

use axum::http::StatusCode;

use argus_protocol::{AgentId, AgentRecord, ThinkingConfig};
use argus_server::response::MutationResponse;

#[tokio::test]
async fn template_routes_create_list_and_update() {
    let ctx = support::TestContext::new().await;

    let initial_response = ctx.get("/api/v1/agents/templates").await;
    assert_eq!(initial_response.status(), StatusCode::OK);
    let initial: Vec<AgentRecord> = support::json_body(initial_response).await;
    let initial_len = initial.len();

    let create_response = ctx
        .post_json(
            "/api/v1/agents/templates",
            &AgentRecord {
                id: AgentId::new(42),
                display_name: "Admin Agent".to_string(),
                description: "created via rest".to_string(),
                version: "1.0.0".to_string(),
                provider_id: None,
                model_id: Some("alpha".to_string()),
                system_prompt: "You are the admin agent.".to_string(),
                tool_names: vec!["shell".to_string()],
                subagent_names: vec![],
                max_tokens: Some(2048),
                temperature: Some(0.2),
                thinking_config: Some(ThinkingConfig::enabled()),
            },
        )
        .await;

    assert_eq!(create_response.status(), StatusCode::CREATED);
    let created: MutationResponse<AgentRecord> = support::json_body(create_response).await;
    assert_ne!(created.item.id.inner(), 42);
    assert_eq!(created.item.display_name, "Admin Agent");

    let update_response = ctx
        .patch_json(
            &format!("/api/v1/agents/templates/{}", created.item.id.inner()),
            &AgentRecord {
                id: AgentId::new(0),
                display_name: "Admin Agent Updated".to_string(),
                ..created.item.clone()
            },
        )
        .await;

    assert_eq!(update_response.status(), StatusCode::OK);
    let updated: MutationResponse<AgentRecord> = support::json_body(update_response).await;
    assert_eq!(updated.item.id, created.item.id);
    assert_eq!(updated.item.display_name, "Admin Agent Updated");

    let final_response = ctx.get("/api/v1/agents/templates").await;
    assert_eq!(final_response.status(), StatusCode::OK);
    let final_body: Vec<AgentRecord> = support::json_body(final_response).await;
    assert_eq!(final_body.len(), initial_len + 1);
}
