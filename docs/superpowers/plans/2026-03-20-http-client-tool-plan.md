# HTTP Client Tool Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a general-purpose HTTP client tool (`http`) to the tool system, with a workspace-shared reqwest client in `argus-protocol`.

**Architecture:**
- Workspace-level `reqwest`, `once_cell`, `url` dependencies in root `Cargo.toml`
- Shared `HTTP_CLIENT` singleton in `crates/argus-protocol/src/http_client.rs`
- New `HttpTool` in `crates/argus-tool/src/http.rs` implementing `NamedTool`
- `HttpTool` registered as default, gated by approval policy

**Tech Stack:** Rust, reqwest, once_cell, url

---

## Chunk 1: Workspace Dependencies + Protocol Foundation

> **Prerequisite for all later chunks.** Must complete before anything else.

### Task 1: Add workspace dependencies to root `Cargo.toml`

**File:** `Cargo.toml` (root)

- [ ] **Step 1: Add `[workspace.dependencies]` section**

Add this section after line 3 (after `resolver = "3"`):

```toml
[workspace.dependencies]
reqwest = { version = "0.12", default-features = false, features = ["rustls-tls-native-roots"] }
once_cell = "1"
url = "2"
```

Run: `cargo build --all` — expect: compile starts (may have warnings about unused deps)

- [ ] **Step 2: Commit**

```bash
git add Cargo.toml
git commit -m "chore: add reqwest, once_cell, url to workspace.dependencies"
```

### Task 2: Update `crates/argus-protocol/Cargo.toml`

**File:** `crates/argus-protocol/Cargo.toml`

- [ ] **Step 1: Replace version strings with workspace references**

Replace line 9: `dashmap = "5"` → `dashmap = { workspace = true }`
Replace line 17: `uuid = { version = "1", features = ["serde", "v4"] }` → `uuid = { workspace = true }`

Add two new dependency lines:
```toml
once_cell = { workspace = true }
reqwest = { workspace = true }
```

Remove line 10 (`futures-core = "0.3"`) — it is not used in argus-protocol.

Run: `cargo build -p argus-protocol` — expect: compile success

- [ ] **Step 2: Commit**

```bash
git add crates/argus-protocol/Cargo.toml
git commit -m "chore(argus-protocol): use workspace deps + add reqwest, once_cell"
```

### Task 3: Create `crates/argus-protocol/src/http_client.rs`

**File:** `crates/argus-protocol/src/http_client.rs` (new)

- [ ] **Step 1: Write the file**

```rust
//! Shared HTTP client with connection pooling.

use once_cell::sync::Lazy;
use reqwest::Client;

/// Shared HTTP client with connection pooling.
/// All crates that need HTTP should use this singleton to share the connection pool.
pub static HTTP_CLIENT: Lazy<Client> = Lazy::new(|| {
    Client::builder()
        .pool_max_idle_per_host(20)
        .use_rustls_tls()
        .build()
        .expect("failed to build HTTP client")
});
```

- [ ] **Step 2: Export from lib.rs**

**File:** `crates/argus-protocol/src/lib.rs`

Add after line 7:
```rust
pub mod http_client;
```

Run: `cargo build -p argus-protocol` — expect: compile success

- [ ] **Step 3: Commit**

```bash
git add crates/argus-protocol/src/http_client.rs crates/argus-protocol/src/lib.rs
git commit -m "feat(argus-protocol): add shared HTTP client singleton"
```

---

## Chunk 2: HttpTool Implementation in argus-tool

### Task 4: Update `crates/argus-tool/Cargo.toml`

**File:** `crates/argus-tool/Cargo.toml`

- [ ] **Step 1: Add dependencies**

Add after line 15:
```toml
reqwest = { workspace = true }
once_cell = { workspace = true }
url = { workspace = true }
```

Run: `cargo build -p argus-tool` — expect: compile success

- [ ] **Step 2: Commit**

```bash
git add crates/argus-tool/Cargo.toml
git commit -m "chore(argus-tool): add reqwest, once_cell, url deps"
```

### Task 5: Write `crates/argus-tool/src/http.rs`

**File:** `crates/argus-tool/src/http.rs` (new)

- [ ] **Step 1: Write the failing test**

```rust
//! HTTP client tool.

use async_trait::async_trait;
use argus_protocol::http_client::HTTP_CLIENT;
use argus_protocol::llm::ToolDefinition;
use argus_protocol::risk_level::RiskLevel;
use argus_protocol::tool::{NamedTool, ToolError};
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};
use serde::Deserialize;
use url::Url;

const MAX_RESPONSE_SIZE: u64 = 10 * 1024 * 1024; // 10MB
const ALLOWED_METHODS: &[&str] = &["GET", "POST", "PUT", "DELETE", "PATCH", "HEAD"];

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct HttpArgs {
    url: String,
    #[serde(default = "default_method")]
    method: String,
    #[serde(default)]
    headers: std::collections::HashMap<String, String>,
    #[serde(default)]
    body: Option<String>,
    #[serde(default = "default_timeout")]
    timeout: u64,
}

fn default_method() -> String {
    "GET".to_string()
}

fn default_timeout() -> u64 {
    30
}

#[derive(Debug, serde::Serialize)]
struct HttpResult {
    status: u16,
    status_text: String,
    headers: std::collections::HashMap<String, Vec<String>>,
    body: String,
}

pub struct HttpTool;

impl HttpTool {
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for HttpTool {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl NamedTool for HttpTool {
    fn name(&self) -> &str {
        "http"
    }

    fn definition(&self) -> ToolDefinition {
        ToolDefinition {
            name: "http".to_string(),
            description: "Make HTTP requests to any URL. Returns status, headers, and body.".to_string(),
            parameters: serde_json::json!({
                "type": "object",
                "properties": {
                    "url": {
                        "type": "string",
                        "description": "Target URL (http/https only)",
                    },
                    "method": {
                        "type": "string",
                        "description": "HTTP method (GET/POST/PUT/DELETE/PATCH/HEAD). Default: GET",
                        "default": "GET",
                    },
                    "headers": {
                        "type": "object",
                        "description": "HTTP headers as key-value pairs",
                        "additionalProperties": { "type": "string" },
                    },
                    "body": {
                        "type": "string",
                        "description": "Request body (sent as-is). Only for POST/PUT/PATCH.",
                    },
                    "timeout": {
                        "type": "integer",
                        "description": "Timeout in seconds. Default: 30, max: 300",
                        "default": 30,
                    },
                },
                "required": ["url"],
            }),
        }
    }

    fn risk_level(&self) -> RiskLevel {
        RiskLevel::Critical
    }

    async fn execute(&self, args: serde_json::Value) -> Result<serde_json::Value, ToolError> {
        let args: HttpArgs = serde_json::from_value(args)
            .map_err(|e| ToolError::ExecutionFailed {
                tool_name: "http",
                reason: format!("invalid arguments: {e}"),
            })?;

        // -- URL validation --
        let parsed_url = Url::parse(&args.url).map_err(|e| ToolError::ExecutionFailed {
            tool_name: "http",
            reason: format!("invalid URL: {e}"),
        })?;

        match parsed_url.scheme() {
            "http" | "https" => {}
            scheme => {
                return Err(ToolError::ExecutionFailed {
                    tool_name: "http",
                    reason: format!(
                        "Unsupported URL scheme '{scheme}'. Only http and https are allowed."
                    ),
                });
            }
        }

        // -- Method validation --
        let method_upper = args.method.to_uppercase();
        if !ALLOWED_METHODS.contains(&method_upper.as_str()) {
            return Err(ToolError::ExecutionFailed {
                tool_name: "http",
                reason: format!("Unsupported HTTP method: '{}'. Allowed: GET, POST, PUT, DELETE, PATCH, HEAD", args.method),
            });
        }

        let reqwest_method = reqwest::Method::from_bytes(method_upper.as_bytes())
            .map_err(|_| ToolError::ExecutionFailed {
                tool_name: "http",
                reason: format!("Unsupported HTTP method: '{}'", args.method),
            })?;

        // -- Timeout clamping --
        let timeout_secs = args.timeout.clamp(1, 300);

        // -- Build headers --
        let mut header_map = HeaderMap::new();
        for (key, value) in &args.headers {
            let header_name = HeaderName::from_bytes(key.as_bytes()).map_err(|_| {
                ToolError::ExecutionFailed {
                    tool_name: "http",
                    reason: format!("invalid header name: {key}"),
                }
            })?;
            let header_value = HeaderValue::from_str(value).map_err(|_| {
                ToolError::ExecutionFailed {
                    tool_name: "http",
                    reason: format!("invalid header value for '{key}'"),
                }
            })?;
            header_map.insert(header_name, header_value);
        }

        // -- Build request --
        let client = HTTP_CLIENT.clone();
        let mut request = client.request(reqwest_method, args.url).headers(header_map);

        if let Some(body) = args.body {
            request = request.body(body);
        }

        request = request.timeout(std::time::Duration::from_secs(timeout_secs));

        // -- Send --
        let response = request
            .send()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool_name: "http",
                reason: if e.is_timeout() {
                    format!("request timed out after {timeout_secs}s")
                } else {
                    format!("request failed: {e}")
                },
            })?;

        // -- Response size check --
        if let Some(len) = response.content_length() {
            if len > MAX_RESPONSE_SIZE {
                return Err(ToolError::ExecutionFailed {
                    tool_name: "http",
                    reason: "Response body too large (max 10MB)".to_string(),
                });
            }
        }

        // -- Collect response --
        let status = response.status();
        let status_text = status.canonical_reason().unwrap_or("Unknown").to_string();

        let mut response_headers = std::collections::HashMap::new();
        for (key, value) in response.headers() {
            response_headers
                .entry(key.as_str().to_lowercase())
                .or_insert_with(Vec::new)
                .push(value.to_str().unwrap_or("").to_string());
        }

        let body = response
            .text()
            .await
            .map_err(|e| ToolError::ExecutionFailed {
                tool_name: "http",
                reason: format!("failed to read response body: {e}"),
            })?;

        let result = HttpResult {
            status: status.as_u16(),
            status_text,
            headers: response_headers,
            body,
        };

        Ok(serde_json::to_value(result).expect("failed to serialize HTTP result"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_unsupported_scheme() {
        let tool = HttpTool::new();
        let result = tool
            .execute(serde_json::json!({
                "url": "file:///etc/passwd",
                "method": "GET"
            }))
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::ExecutionFailed { ref reason, .. }
            if reason.contains("Unsupported URL scheme")));
    }

    #[tokio::test]
    async fn test_unsupported_method() {
        let tool = HttpTool::new();
        let result = tool
            .execute(serde_json::json!({
                "url": "https://example.com",
                "method": "CONNECT"
            }))
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::ExecutionFailed { ref reason, .. }
            if reason.contains("Unsupported HTTP method")));
    }

    #[tokio::test]
    async fn test_invalid_url() {
        let tool = HttpTool::new();
        let result = tool
            .execute(serde_json::json!({
                "url": "not a valid url at all",
                "method": "GET"
            }))
            .await;
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ToolError::ExecutionFailed { ref reason, .. }
            if reason.contains("invalid URL")));
    }

    #[test]
    fn tool_name_is_http() {
        let tool = HttpTool::new();
        assert_eq!(tool.name(), "http");
    }

    #[test]
    fn tool_risk_level_is_critical() {
        let tool = HttpTool::new();
        assert_eq!(tool.risk_level(), RiskLevel::Critical);
    }

    #[test]
    fn tool_definition_has_url_required() {
        let tool = HttpTool::new();
        let def = tool.definition();
        assert_eq!(def.name, "http");
        // Verify url is required by checking the schema contains "url" in required array
        let params = &def.parameters;
        let required = params.get("required").and_then(|r| r.as_array());
        assert!(required.map_or(false, |r| r.iter().any(|v| v == "url")));
    }
}
```

Run: `cargo test -p argus-tool http` — expect: compile + tests pass (some tests may fail due to network, that's OK for unit tests that test validation)

- [ ] **Step 2: Run tests**

Run: `cargo test -p argus-tool -- http --test-threads=1` — expect: 4 compile + 6 tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/argus-tool/src/http.rs
git commit -m "feat(argus-tool): add HttpTool for general-purpose HTTP requests"
```

### Task 6: Export `HttpTool` from `argus-tool`

**File:** `crates/argus-tool/src/lib.rs`

- [ ] **Step 1: Add module and re-export**

Add after line 18:
```rust
pub mod http;
```

Change line 21 from:
```rust
pub use glob::GlobTool;
```
to:
```rust
pub use glob::GlobTool;
pub use http::HttpTool;
```

Run: `cargo build -p argus-tool` — expect: compile success

- [ ] **Step 2: Commit**

```bash
git add crates/argus-tool/src/lib.rs
git commit -m "feat(argus-tool): export HttpTool"
```

---

## Chunk 3: Registration + Cleanup + Verification

### Task 7: Update approval policy defaults

**File:** `crates/argus-approval/src/policy.rs`

- [ ] **Step 1: Add `"http"` to default require_approval**

Change line 48 from:
```rust
require_approval: vec!["shell_exec".to_string()],
```
to:
```rust
require_approval: vec!["shell".to_string(), "http".to_string()],
```

Update the comment on line 29 from `"shell_exec"` to `"shell", "http"`.

Also update `visit_bool` on line 76: `vec!["shell_exec".to_string()]` → `vec!["shell".to_string(), "http".to_string()]`

- [ ] **Step 2: Update test assertions**

Change line 167: `assert_eq!(policy.require_approval, vec!["shell_exec".to_string()]);`
→ `assert_eq!(policy.require_approval, vec!["shell".to_string(), "http".to_string()]);`

Change line 177: same update.

Change line 191: same update.

Change line 197: `assert!(policy.requires_approval("shell_exec"));`
→ `assert!(policy.requires_approval("shell"));`

Add after line 198:
```rust
assert!(policy.requires_approval("http"));
```

Change line 246: `policy.require_approval = vec!["shell_exec".into(), "".into()];`
→ `policy.require_approval = vec!["http".into(), "".into()];`

Change line 301: `"shell_exec".into()` → `"shell".into()` (2 occurrences)

Run: `cargo test -p argus-approval` — expect: all tests pass

- [ ] **Step 3: Commit**

```bash
git add crates/argus-approval/src/policy.rs
git commit -m "feat(argus-approval): gate http tool by default (with shell)"
```

### Task 8: Register `HttpTool` as default in `argus-wing`

**File:** `crates/argus-wing/src/lib.rs`

- [ ] **Step 1: Update register_default_tools**

Change lines 267-272 from:
```rust
use argus_tool::{GlobTool, GrepTool, ReadTool, ShellTool};

self.tool_manager.register(Arc::new(ShellTool::new()));
self.tool_manager.register(Arc::new(ReadTool::new()));
self.tool_manager.register(Arc::new(GrepTool::new()));
self.tool_manager.register(Arc::new(GlobTool::new()));
```
to:
```rust
use argus_tool::{GlobTool, GrepTool, HttpTool, ReadTool, ShellTool};

self.tool_manager.register(Arc::new(ShellTool::new()));
self.tool_manager.register(Arc::new(ReadTool::new()));
self.tool_manager.register(Arc::new(GrepTool::new()));
self.tool_manager.register(Arc::new(GlobTool::new()));
self.tool_manager.register(Arc::new(HttpTool::new()));
```

Also update the doc comment on line 265 from:
`/// Register default tools (shell, read, grep, glob) with the tool manager.`
to:
`/// Register default tools (shell, read, grep, glob, http) with the tool manager.`

Run: `cargo build -p argus-wing` — expect: compile success

- [ ] **Step 2: Commit**

```bash
git add crates/argus-wing/src/lib.rs
git commit -m "feat(argus-wing): register HttpTool as default"
```

### Task 9: Update `agents/arguswing.toml`

**File:** `agents/arguswing.toml`

- [ ] **Step 1: Add "http" to tool_names**

Change line 5 from:
```toml
tool_names = ["shell", "read", "grep", "glob"]
```
to:
```toml
tool_names = ["shell", "read", "grep", "glob", "http"]
```

- [ ] **Step 2: Commit**

```bash
git add agents/arguswing.toml
git commit -m "feat(agents): add http tool to arguswing agent"
```

### Task 10: Cleanup unused `argus-llm/http_client.rs`

**File:** `crates/argus-llm/src/http_client.rs`

- [ ] **Step 1: Verify the file is unused**

Check `crates/argus-llm/src/lib.rs` — confirm there is no `pub mod http_client;` or `mod http_client;`.
Check `crates/argus-llm/src/providers/openai_compatible.rs` — confirm it creates its own `reqwest::Client`, does not import from http_client.

Run: `grep -r "http_client" crates/argus-llm/src/` — expect: no matches referencing the local file

- [ ] **Step 2: Delete the file**

Bash: `rm crates/argus-llm/src/http_client.rs`

- [ ] **Step 3: Update argus-llm Cargo.toml to use workspace reqwest**

**File:** `crates/argus-llm/Cargo.toml`

Replace line 25:
```toml
reqwest = { version = "0.12", default-features = false, features = ["json", "stream", "rustls-tls-native-roots"] }
```
with:
```toml
reqwest = { workspace = true, features = ["json", "stream"] }
```

Run: `cargo build -p argus-llm` — expect: compile success

- [ ] **Step 4: Commit**

```bash
git add crates/argus-llm/
git rm crates/argus-llm/src/http_client.rs
git commit -m "chore(argus-llm): remove unused http_client, use workspace reqwest"
```

### Task 11: Update remaining crates to use workspace deps

**File:** Check which crates still have hardcoded `reqwest`, `once_cell`, or `url` deps.

- [ ] **Step 1: Find crates using reqwest/once_cell**

Run: `grep -r 'reqwest\|once_cell' crates/*/Cargo.toml | grep -v workspace`

Expected results (may vary based on current state):
- `crates/argus-auth/Cargo.toml` — may have reqwest
- `crates/argus-llm/Cargo.toml` — should now use workspace
- Any others: update to `workspace = true`

For each found:
Replace version string with `{ workspace = true }` or `{ workspace = true, features = [...] }` if it has extra features.

Run: `cargo build --all` — expect: all compile

- [ ] **Step 2: Commit**

```bash
git add crates/*/Cargo.toml
git commit -m "chore: migrate remaining crates to workspace deps"
```

### Task 12: Full verification

- [ ] **Step 1: Build all**

Run: `cargo build --all` — expect: all crates compile

- [ ] **Step 2: Run all tests**

Run: `cargo test --all` — expect: all tests pass (3 pre-existing failures in argus-wing are known)

- [ ] **Step 3: Clippy**

Run: `cargo clippy --all` — expect: no warnings

- [ ] **Step 4: Fmt**

Run: `cargo fmt` — expect: no changes needed (or applied if pre-commit hook)

- [ ] **Step 5: Final commit**

```bash
git add -A
git commit -m "feat: add HTTP client tool with workspace-shared reqwest

- Add workspace-shared HTTP client singleton in argus-protocol
- Implement HttpTool (GET/POST/PUT/DELETE/PATCH/HEAD) in argus-tool
- Register HttpTool as default with Critical risk level
- Gate http tool by default in approval policy
- Add http to arguswing agent tool_names
- Cleanup unused argus-llm/http_client.rs
- Migrate all crates to workspace dependencies"
```
