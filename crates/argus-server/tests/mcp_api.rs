mod support;

use std::collections::BTreeMap;

use axum::http::StatusCode;

use argus_protocol::{McpServerRecord, McpServerStatus, McpTransportConfig};
use argus_server::response::MutationResponse;

#[tokio::test]
async fn mcp_routes_create_list_and_update() {
    let ctx = support::TestContext::new().await;

    let initial_response = ctx.get("/api/v1/mcp/servers").await;
    assert_eq!(initial_response.status(), StatusCode::OK);
    let initial: Vec<McpServerRecord> = support::json_body(initial_response).await;
    let initial_len = initial.len();

    let create_response = ctx
        .post_json(
            "/api/v1/mcp/servers",
            &McpServerRecord {
                id: Some(51),
                display_name: "Docs MCP".to_string(),
                enabled: true,
                transport: McpTransportConfig::Http {
                    url: "https://example.invalid/mcp".to_string(),
                    headers: BTreeMap::new(),
                },
                timeout_ms: 15_000,
                status: McpServerStatus::Connecting,
                last_checked_at: None,
                last_success_at: None,
                last_error: None,
                discovered_tool_count: 0,
            },
        )
        .await;

    assert_eq!(create_response.status(), StatusCode::CREATED);
    let created: MutationResponse<McpServerRecord> = support::json_body(create_response).await;
    assert_ne!(created.item.id, Some(51));
    assert_eq!(created.item.display_name, "Docs MCP");

    let update_response = ctx
        .patch_json(
            &format!(
                "/api/v1/mcp/servers/{}",
                created
                    .item
                    .id
                    .expect("created server should have a persisted id")
            ),
            &McpServerRecord {
                id: None,
                display_name: "Docs MCP Updated".to_string(),
                enabled: false,
                transport: created.item.transport.clone(),
                timeout_ms: 30_000,
                status: McpServerStatus::Disabled,
                last_checked_at: created.item.last_checked_at.clone(),
                last_success_at: created.item.last_success_at.clone(),
                last_error: created.item.last_error.clone(),
                discovered_tool_count: created.item.discovered_tool_count,
            },
        )
        .await;

    assert_eq!(update_response.status(), StatusCode::OK);
    let updated: MutationResponse<McpServerRecord> = support::json_body(update_response).await;
    assert_eq!(updated.item.display_name, "Docs MCP Updated");
    assert_eq!(updated.item.status, McpServerStatus::Disabled);

    let final_response = ctx.get("/api/v1/mcp/servers").await;
    assert_eq!(final_response.status(), StatusCode::OK);
    let final_body: Vec<McpServerRecord> = support::json_body(final_response).await;
    assert_eq!(final_body.len(), initial_len + 1);
}
