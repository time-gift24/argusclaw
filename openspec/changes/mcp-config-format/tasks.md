# Implementation Tasks

## 1. Database Migration

- [x] 1.1 Create migration file `3__update_mcp_servers_config.sql`:
  - Rename `transport` column to `server_type` (TEXT: "http" or "stdio")
  - Add `url` column (TEXT, nullable) for HTTP type
  - Add `headers` column (TEXT JSON, nullable) for HTTP type headers
  - Add `args` column (TEXT JSON, nullable) for Stdio type args
  - Keep `command` column (TEXT, nullable) for Stdio type
- [x] 1.2 Add migration script to convert existing data:
  - `transport = 'Stdio'` → `server_type = 'Stdio'`, `command` from original field
  - `transport = 'SSE'` → `server_type = 'Http'`, `url` from original field

## 2. Protocol Layer (argus-protocol)

- [x] 2.1 Update `TransportType` enum to `ServerType` enum with `Http` and `Stdio` variants
- [x] 2.2 Update `McpServerConfig` struct to use new fields:
  - `server_type: ServerType` (replaces `transport: TransportType`)
  - `url: Option<String>` for HTTP type
  - `headers: Option<HashMap<String, String>>` for HTTP type
  - `args: Option<Vec<String>>` for Stdio type
- [x] 2.3 Add `McpServerConfigJson` for frontend serialization
- [x] 2.4 Export updated types from `argus-protocol/src/mcp/mod.rs`

## 3. Database Repository (argus-repository)

- [x] 3.1 Update `McpServerRecord` struct to use new fields
- [x] 3.2 Update `McpServerRepository` trait methods
- [x] 3.3 Update SQLite implementation for new schema

## 4. MCP Client Module (argus-tool)

- [x] 4.1 Update `McpClientWrapper::new()` to use new config fields:
  - HTTP: use `url` and `headers` for `ClientSseTransport`
  - Stdio: use `command` and `args` for `StdioTransport`
- [x] 4.2 Update `ConnectionTestResult` serialization if needed

## 5. API Layer (argus-wing)

- [x] 5.1 Update `register_mcp_tools()` to use new config format
- [x] 5.2 Update MCP CRUD methods signatures

## 6. Tauri Commands (desktop)

- [x] 6.1 Update `McpServerPayload` struct with new fields
- [x] 6.2 Update Tauri commands to use new payload format

## 7. Frontend (desktop)

- [x] 7.1 Update `lib/tauri.ts` types and `mcpServers` API:
  - `TransportType` → `ServerType` ("http" | "stdio")
  - Add `url`, `headers`, `args` fields
- [x] 7.2 Update `mcp-server-form-dialog.tsx`:
  - Add `url` field for HTTP type
  - Add `headers` field for HTTP type
  - Add `args` field for Stdio type
  - Rename `transport` to `server_type`
- [x] 7.3 Update `mcp-server-card.tsx` to display new fields

## 8. Data Migration (runtime)

- [ ] 8.1 Create migration logic to convert existing database records
- [ ] 8.2 Handle backward compatibility during transition
