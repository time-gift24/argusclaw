# Implementation Tasks

## 1. Protocol Layer (argus-protocol)

- [x] 1.1 Add `McpServerConfig` struct with fields: id, name, display_name, transport, command, url, enabled
- [x] 1.2 Add `TransportType` enum with `Stdio` and `Sse` variants
- [x] 1.3 Add `ToolError::McpToolError` variant with server, tool, context, source fields
- [x] 1.4 Add `mcp` module to lib.rs exports

## 2. Database Migration (argus-repository)

- [x] 2.1 Create migration file `2__add_mcp_servers.sql` with mcp_servers table
- [x] 2.2 Add `mcp_server` table CRUD operations to ArgusSqlite

## 3. MCP Client Module (argus-tool)

- [x] 3.1 Create `crates/argus-tool/src/mcp/mod.rs` with module structure
- [x] 3.2 Implement `McpClientPool` with connection management
- [x] 3.3 Implement `McpTool` wrapping `ClientRuntime` as `NamedTool`
- [x] 3.4 Implement tool discovery (list_tools from MCP server)
- [x] 3.5 Implement `mcp_{server}_{tool}` naming convention
- [x] 3.6 Add Stdio transport support via `StdioTransport`
- [x] 3.7 Add SSE transport support via `ClientSseTransport`
- [x] 3.8 Implement connection test function with timeout

## 4. Repository Trait (argus-repository)

- [x] 4.1 Add `McpServerRepository` trait to `argus-repository/src/traits/`
- [x] 4.2 Implement `McpServerRepository` for `ArgusSqlite`

## 5. API Layer (argus-wing + desktop)

- [x] 5.1 Add `McpServerRepository` to `ArgusWing` for DI
- [x] 5.2 Add async `register_mcp_tools()` method to `ArgusWing` - loads enabled MCP servers, connects, discovers tools, registers to ToolManager
- [x] 5.3 Add MCP CRUD methods to `ArgusWing`: list_mcp_servers, get_mcp_server, upsert_mcp_server, delete_mcp_server
- [x] 5.4 Add test_connection method to `ArgusWing`
- [x] 5.5 Add Tauri commands: list_mcp_servers, get_mcp_server, upsert_mcp_server, delete_mcp_server, test_mcp_server
- [x] 5.6 Register Tauri commands in desktop/src-tauri/src/lib.rs
- [x] 5.7 Call `wing.register_mcp_tools()` after `register_default_tools()` in desktop startup

## 6. Frontend (desktop)

- [x] 6.1 Create `/settings/mcp` route with layout
- [x] 6.2 Implement MCP Server list component
- [x] 6.3 Implement Add/Edit MCP Server modal with form
- [x] 6.4 Implement Test Connection button with status feedback
- [x] 6.5 Implement Delete confirmation dialog
- [x] 6.6 Add MCP tools to Agent edit page tool selection
- [x] 6.7 Add MCP servers to settings sidebar navigation

## 7. Integration & Testing

- [x] 7.1 Add `rust-mcp-sdk` dependency to Cargo.toml
- [x] 7.2 Verify build with `cargo build`
- [x] 7.3 Run tests to ensure no regressions
- [ ] 7.4 Manual test: configure filesystem MCP server, verify tools appear in agent
