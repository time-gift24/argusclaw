mod support;

use std::collections::{BTreeMap, HashMap};

use axum::http::StatusCode;

use argus_protocol::llm::{ModelConfig, ProviderTestResult, ProviderTestStatus};
use argus_protocol::{
    AgentId, AgentRecord, LlmProviderKind, LlmProviderRecordJson, McpDiscoveredToolRecord,
    McpServerRecord, McpServerStatus, McpTransportConfig, ProviderSecretStatus, ThinkingConfig,
};
use argus_server::response::MutationResponse;
use argus_server::routes::templates::{AgentMcpBindingPayload, TemplateRecordPayload};

fn provider_record(display_name: &str) -> LlmProviderRecordJson {
    LlmProviderRecordJson {
        id: 0,
        kind: LlmProviderKind::OpenAiCompatible,
        display_name: display_name.to_string(),
        base_url: "http://127.0.0.1:1/v1".to_string(),
        api_key: "sk-test".to_string(),
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
        meta_data: HashMap::from([("timeout_secs".to_string(), "1".to_string())]),
    }
}

fn template_record(display_name: &str) -> AgentRecord {
    AgentRecord {
        id: AgentId::new(0),
        display_name: display_name.to_string(),
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
    }
}

fn template_payload(
    display_name: &str,
    mcp_bindings: Vec<AgentMcpBindingPayload>,
) -> TemplateRecordPayload {
    TemplateRecordPayload {
        record: template_record(display_name),
        mcp_bindings,
    }
}

fn mcp_server(display_name: &str) -> McpServerRecord {
    McpServerRecord {
        id: None,
        display_name: display_name.to_string(),
        enabled: true,
        transport: McpTransportConfig::Stdio {
            command: "definitely-missing-mcp-binary".to_string(),
            args: Vec::new(),
            env: BTreeMap::new(),
        },
        timeout_ms: 250,
        status: McpServerStatus::Connecting,
        last_checked_at: None,
        last_success_at: None,
        last_error: None,
        discovered_tool_count: 0,
    }
}

#[tokio::test]
async fn provider_delete_and_test_routes_complete_management_loop() {
    let ctx = support::TestContext::new().await;
    let create_response = ctx
        .post_json("/api/v1/providers", &provider_record("Delete Me"))
        .await;
    assert_eq!(create_response.status(), StatusCode::CREATED);
    let created: MutationResponse<LlmProviderRecordJson> =
        support::json_body(create_response).await;

    let persisted_test_response = ctx
        .post_json(
            &format!("/api/v1/providers/{}/test", created.item.id),
            &serde_json::json!({ "model": "alpha" }),
        )
        .await;
    assert_eq!(persisted_test_response.status(), StatusCode::OK);
    let persisted_test: ProviderTestResult = support::json_body(persisted_test_response).await;
    assert_eq!(persisted_test.model, "alpha");
    assert!(matches!(
        persisted_test.status,
        ProviderTestStatus::RequestFailed | ProviderTestStatus::RateLimited
    ));

    let unsaved_test_response = ctx
        .post_json(
            "/api/v1/providers/test",
            &provider_record("Unsaved Provider"),
        )
        .await;
    assert_eq!(unsaved_test_response.status(), StatusCode::OK);
    let unsaved_test: ProviderTestResult = support::json_body(unsaved_test_response).await;
    assert_eq!(unsaved_test.model, "alpha");

    let delete_response = ctx
        .delete(&format!("/api/v1/providers/{}", created.item.id))
        .await;
    assert_eq!(delete_response.status(), StatusCode::OK);

    let list_response = ctx.get("/api/v1/providers").await;
    let providers: Vec<LlmProviderRecordJson> = support::json_body(list_response).await;
    assert!(
        providers
            .iter()
            .all(|provider| provider.display_name != "Delete Me")
    );
}

#[tokio::test]
async fn template_delete_route_removes_template() {
    let ctx = support::TestContext::new().await;
    let create_response = ctx
        .post_json(
            "/api/v1/agents/templates",
            &template_record("Disposable Agent"),
        )
        .await;
    assert_eq!(create_response.status(), StatusCode::CREATED);
    let created: MutationResponse<AgentRecord> = support::json_body(create_response).await;

    let delete_response = ctx
        .delete(&format!(
            "/api/v1/agents/templates/{}",
            created.item.id.inner()
        ))
        .await;
    assert_eq!(delete_response.status(), StatusCode::OK);

    let list_response = ctx.get("/api/v1/agents/templates").await;
    let templates: Vec<AgentRecord> = support::json_body(list_response).await;
    assert!(
        templates
            .iter()
            .all(|template| template.display_name != "Disposable Agent")
    );
}

#[tokio::test]
async fn template_routes_round_trip_mcp_bindings() {
    let ctx = support::TestContext::new().await;
    let server_response = ctx
        .post_json("/api/v1/mcp/servers", &mcp_server("Template Slack"))
        .await;
    assert_eq!(server_response.status(), StatusCode::CREATED);
    let created_server: MutationResponse<McpServerRecord> =
        support::json_body(server_response).await;
    let server_id = created_server
        .item
        .id
        .expect("created server should have id");

    let create_response = ctx
        .post_json(
            "/api/v1/agents/templates",
            &template_payload(
                "MCP Enabled Agent",
                vec![AgentMcpBindingPayload {
                    server_id,
                    allowed_tools: None,
                }],
            ),
        )
        .await;
    assert_eq!(create_response.status(), StatusCode::CREATED);
    let created: MutationResponse<TemplateRecordPayload> =
        support::json_body(create_response).await;

    assert_eq!(created.item.record.display_name, "MCP Enabled Agent");
    assert_eq!(created.item.mcp_bindings.len(), 1);
    assert_eq!(created.item.mcp_bindings[0].server_id, server_id);
    assert_eq!(created.item.mcp_bindings[0].allowed_tools, None);

    let list_response = ctx.get("/api/v1/agents/templates").await;
    let templates: Vec<TemplateRecordPayload> = support::json_body(list_response).await;
    let found = templates
        .into_iter()
        .find(|template| template.record.display_name == "MCP Enabled Agent")
        .expect("template should be listed");
    assert_eq!(found.mcp_bindings.len(), 1);
    assert_eq!(found.mcp_bindings[0].server_id, server_id);
}

#[tokio::test]
async fn mcp_delete_test_and_tools_routes_complete_management_loop() {
    let ctx = support::TestContext::new().await;
    let create_response = ctx
        .post_json("/api/v1/mcp/servers", &mcp_server("Disposable MCP"))
        .await;
    assert_eq!(create_response.status(), StatusCode::CREATED);
    let created: MutationResponse<McpServerRecord> = support::json_body(create_response).await;
    let server_id = created.item.id.expect("created server should have id");

    ctx.seed_mcp_tools(
        server_id,
        vec![McpDiscoveredToolRecord {
            server_id,
            tool_name_original: "search_docs".to_string(),
            description: "Search docs".to_string(),
            schema: serde_json::json!({ "type": "object" }),
            annotations: None,
        }],
    )
    .await;

    let tools_response = ctx
        .get(&format!("/api/v1/mcp/servers/{server_id}/tools"))
        .await;
    assert_eq!(tools_response.status(), StatusCode::OK);
    let tools: Vec<McpDiscoveredToolRecord> = support::json_body(tools_response).await;
    assert_eq!(tools.len(), 1);
    assert_eq!(tools[0].tool_name_original, "search_docs");

    let persisted_test_response = ctx
        .post_empty(&format!("/api/v1/mcp/servers/{server_id}/test"))
        .await;
    assert_eq!(persisted_test_response.status(), StatusCode::OK);

    let unsaved_test_response = ctx
        .post_json("/api/v1/mcp/servers/test", &mcp_server("Unsaved MCP"))
        .await;
    assert_eq!(unsaved_test_response.status(), StatusCode::OK);

    let delete_response = ctx
        .delete(&format!("/api/v1/mcp/servers/{server_id}"))
        .await;
    assert_eq!(delete_response.status(), StatusCode::OK);

    let list_response = ctx.get("/api/v1/mcp/servers").await;
    let servers: Vec<McpServerRecord> = support::json_body(list_response).await;
    assert!(
        servers
            .iter()
            .all(|server| server.display_name != "Disposable MCP")
    );
}
