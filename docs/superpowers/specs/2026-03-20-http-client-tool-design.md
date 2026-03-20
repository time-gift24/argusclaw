# Design: HTTP Client Tool

## Status

Approved — Pending Implementation

## Overview

Add a general-purpose HTTP client tool (`http`) to the argus tool system, allowing LLM agents to make arbitrary HTTP requests. This extends the existing filesystem-only tool suite with network access capability.

## Architecture

### 1. Shared HTTP Client Module

**File**: `crates/argus-protocol/src/http_client.rs` (new)

A workspace-shared lazy-initialized `reqwest::Client` with connection pooling. This module is consumed by both `argus-llm` (existing usage) and `argus-tool` (new usage), avoiding duplicate client instances.

```rust
use once_cell::sync::Lazy;
use reqwest::Client;

pub static HTTP_CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .pool_max_idle_per_host(20)
        .use_rustls_tls()
        .build()
        .expect("failed to build HTTP client")
});
```

**Reqwest features**:
```
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls-native-roots"] }
```
`rustls-tls-native-roots` enables TLS with the OS trust store (same as existing `argus-llm` usage). No `json` feature needed — we handle body parsing manually.

**Migration**: `crates/argus-llm/src/http_client.rs` is currently unused (the `OpenAiCompatibleProvider` creates its own `reqwest::Client`). Delete it and update `argus-llm`'s `Cargo.toml` to remove `reqwest`.

### 2. HttpTool Implementation

**File**: `crates/argus-tool/src/http.rs` (new)

Implements `NamedTool` for a general-purpose HTTP client.

**Parameters** (JSON Schema):

| Parameter | Type    | Required | Default | Description                                             |
|-----------|---------|----------|---------|---------------------------------------------------------|
| `url`     | string  | Yes      | —       | Target URL (http/https only)                            |
| `method`  | string  | No       | `"GET"` | HTTP method (GET/POST/PUT/DELETE/PATCH/HEAD)           |
| `headers` | object  | No       | `{}`    | HTTP headers as key-value pairs                         |
| `body`    | string  | No       | —       | Request body (sent as-is; LLM should JSON-serialize)   |
| `timeout` | integer | No       | `30`    | Timeout in seconds (max 300)                           |

**Returns**:

```json
{
  "status": 200,
  "status_text": "OK",
  "headers": { "content-type": ["application/json"] },
  "body": "..."
}
```

**Risk Level**: `Critical` — can access arbitrary network endpoints.

**Validation rules**:
- `url`: Parsed via `Url::parse`, scheme must be `http` or `https` only. `file://`, `gopher://`, etc. are rejected.
- `method`: Validated against allowlist — `GET`, `POST`, `PUT`, `DELETE`, `PATCH`, `HEAD`. Unrecognized methods return `ToolError::ExecutionFailed`.
- `timeout`: Clamped to `1..=300` range.
- `headers`: Keys and values sent as-is. No blocking of sensitive headers — callers can set `Authorization` as needed.

**Security measures**:
- Scheme allowlist: only `http` and `https` permitted. Internal schemes like `file://` are blocked.
- Response size limit: 10MB max. If `content-length > 10MB`, returns `ToolError::ExecutionFailed` with message.
- Timeout enforced per-request (not global), clamped to user-specified value.

### 3. Default Registration

**File**: `crates/argus-wing/src/lib.rs`

Add `HttpTool` to `register_default_tools()`:

```rust
use argus_tool::HttpTool;
self.tool_manager.register(Arc::new(HttpTool::new()));
```

**Approval Policy**: The default `ApprovalPolicy` in `argus-approval/src/policy.rs` defaults to gating `"shell_exec"` (note: actual `ShellTool::name()` returns `"shell"`, so this is a pre-existing naming mismatch). Add `"http"` to the default `require_approval` list alongside `"shell"`:

```rust
// In ApprovalPolicy::default()
require_approval: vec!["shell".to_string(), "http".to_string()],
```

This ensures every `http` call requires human approval by default, given its `Critical` risk level.

**Template file**: `agents/arguswing.toml` (NOT `crates/argus-template/data/agents/`) — add `"http"` to `tool_names`.

## Changes Summary

| File | Change |
|------|--------|
| `Cargo.toml` (root) | Add `reqwest`, `once_cell` to `workspace.dependencies` |
| `crates/argus-protocol/Cargo.toml` | Add `once_cell`, `reqwest` (workspace) |
| `crates/argus-protocol/src/http_client.rs` | NEW — shared lazy HTTP client |
| `crates/argus-protocol/src/lib.rs` | Export `http_client` module |
| `crates/argus-llm/Cargo.toml` | Remove `reqwest`, `once_cell` (unused) |
| `crates/argus-llm/src/http_client.rs` | DELETE — unused file |
| `crates/argus-llm/src/lib.rs` | Re-export from `argus_protocol::http_client` if needed |
| `crates/argus-tool/Cargo.toml` | Add `reqwest`, `once_cell`, `url` (workspace) |
| `crates/argus-tool/src/http.rs` | NEW — HttpTool implementation |
| `crates/argus-tool/src/lib.rs` | Export `HttpTool` |
| `crates/argus-approval/src/policy.rs` | Add `"http"` to default `require_approval` |
| `crates/argus-wing/src/lib.rs` | Register `HttpTool` as default |
| `agents/arguswing.toml` | Add `"http"` to `tool_names` |

**New dependency for `argus-tool`**: `url` crate (for URL parsing/validation), added to workspace.

## Error Handling

- Invalid URL → `ToolError::ExecutionFailed` with "invalid URL: {reason}"
- Unsupported scheme → `ToolError::ExecutionFailed` with "Unsupported URL scheme 'x'. Only http and https are allowed."
- Unsupported method → `ToolError::ExecutionFailed` with "Unsupported HTTP method: 'x'"
- Response too large → `ToolError::ExecutionFailed` with "Response body too large (max 10MB)"
- HTTP errors (4xx/5xx) → still return success with status/body, LLM handles retry/fix
- Timeout → `ToolError::ExecutionFailed` with "request timed out after {n}s"
- Network error → `ToolError::ExecutionFailed` with error message

## Testing

- Unit tests for `HttpTool::execute()`:
  - Valid GET request → verify status, headers, body
  - POST with body → verify body sent
  - Unsupported scheme → verify error
  - Unsupported method → verify error
  - Response too large → verify error
  - Invalid URL → verify error
- Integration test via CLI: `arg run` with a simple `http` call to a public API
